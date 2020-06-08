package net.mullvad.mullvadvpn.service

import android.app.KeyguardManager
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import kotlin.properties.Delegates.observable
import net.mullvad.mullvadvpn.model.TunnelState
import net.mullvad.mullvadvpn.service.notifications.TunnelStateNotification
import net.mullvad.talpid.util.EventNotifier

class ForegroundNotificationManager(
    val service: MullvadVpnService,
    val serviceNotifier: EventNotifier<ServiceInstance?>,
    val keyguardManager: KeyguardManager
) {
    private val tunnelStateNotification = TunnelStateNotification(service)

    private val deviceLockListener = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val action = intent.action

            if (action == Intent.ACTION_USER_PRESENT || action == Intent.ACTION_SCREEN_OFF) {
                deviceIsUnlocked = !keyguardManager.isDeviceLocked
            }
        }
    }

    private var connectionProxy by observable<ConnectionProxy?>(null) { _, oldValue, newValue ->
        if (oldValue != newValue) {
            oldValue?.onStateChange?.unsubscribe(this)

            newValue?.onStateChange?.subscribe(this) { state ->
                tunnelState = state
            }
        }
    }

    private var settingsListener by observable<SettingsListener?>(null) { _, oldValue, newValue ->
        if (oldValue != newValue) {
            oldValue?.accountNumberNotifier?.unsubscribe(this)

            newValue?.accountNumberNotifier?.subscribe(this) { accountNumber ->
                loggedIn = accountNumber != null
            }
        }
    }

    private var tunnelState by observable<TunnelState>(TunnelState.Disconnected()) { _, _, state ->
        tunnelStateNotification.tunnelState = state
        updateNotification()
    }

    private var deviceIsUnlocked by observable(!keyguardManager.isDeviceLocked) { _, _, _ ->
        updateNotificationAction()
    }

    private var loggedIn by observable(false) { _, _, _ -> updateNotificationAction() }

    private var onForeground = false

    private val shouldBeOnForeground
        get() = lockedToForeground || !(tunnelState is TunnelState.Disconnected)

    var lockedToForeground by observable(false) { _, _, _ -> updateNotification() }

    init {
        serviceNotifier.subscribe(this) { newServiceInstance ->
            connectionProxy = newServiceInstance?.connectionProxy
            settingsListener = newServiceInstance?.settingsListener
        }

        service.apply {
            registerReceiver(deviceLockListener, IntentFilter().apply {
                addAction(Intent.ACTION_USER_PRESENT)
                addAction(Intent.ACTION_SCREEN_OFF)
            })
        }

        updateNotification()
    }

    fun onDestroy() {
        serviceNotifier.unsubscribe(this)
        connectionProxy = null
        settingsListener = null

        service.unregisterReceiver(deviceLockListener)

        tunnelStateNotification.visible = false
    }

    private fun updateNotification() {
        if (shouldBeOnForeground != onForeground) {
            if (shouldBeOnForeground) {
                service.startForeground(
                    TunnelStateNotification.NOTIFICATION_ID,
                    tunnelStateNotification.build()
                )

                onForeground = true
            } else if (!shouldBeOnForeground) {
                if (Build.VERSION.SDK_INT >= 24) {
                    service.stopForeground(Service.STOP_FOREGROUND_DETACH)
                } else {
                    service.stopForeground(false)
                }

                onForeground = false
            }
        }
    }

    private fun updateNotificationAction() {
        tunnelStateNotification.showAction = loggedIn && deviceIsUnlocked
    }
}
