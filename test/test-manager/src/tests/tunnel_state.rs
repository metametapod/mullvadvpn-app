use super::helpers::{
    self, connect_and_wait, get_tunnel_state, send_guest_probes, set_relay_settings,
    unreachable_wireguard_tunnel, wait_for_tunnel_state,
};
use super::{ui, Error, TestContext};
use crate::assert_tunnel_state;
use crate::vm::network::DUMMY_LAN_INTERFACE_IP;

use mullvad_management_interface::ManagementServiceClient;
use mullvad_types::relay_constraints::GeographicLocationConstraint;
use mullvad_types::relay_list::{Relay, RelayEndpointData};
use mullvad_types::CustomTunnelEndpoint;
use mullvad_types::{
    relay_constraints::{Constraint, LocationConstraint, RelayConstraints, RelaySettings},
    states::TunnelState,
};
use std::net::{IpAddr, SocketAddr};
use talpid_types::net::{Endpoint, TransportProtocol, TunnelEndpoint, TunnelType};
use test_macro::test_function;
use test_rpc::ServiceClient;

/// Verify that outgoing TCP, UDP, and ICMP packets can be observed
/// in the disconnected state. The purpose is mostly to rule prevent
/// false negatives in other tests.
/// This also ensures that the disconnected view is shown in the Electron app.
#[test_function]
pub async fn test_disconnected_state(
    _: TestContext,
    rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    let inet_destination = "1.3.3.7:1337".parse().unwrap();

    log::info!("Verify tunnel state: disconnected");
    assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

    //
    // Test whether outgoing packets can be observed
    //

    log::info!("Sending packets to {inet_destination}");

    let non_tunnel_interface = rpc
        .get_default_interface()
        .await
        .expect("failed to obtain non-tun interface");

    let detected_probes =
        send_guest_probes(rpc.clone(), non_tunnel_interface, inet_destination).await?;
    assert!(
        detected_probes.all(),
        "did not see (all) outgoing packets to destination: {detected_probes:?}",
    );

    //
    // Test UI view
    //

    log::info!("UI: Test disconnected state");
    let ui_result = ui::run_test(&rpc, &["disconnected.spec"]).await.unwrap();
    assert!(ui_result.success());

    Ok(())
}

/// Try to produce leaks in the connecting state by forcing
/// the app into the connecting state and trying to leak,
/// failing if any the following outbound traffic is
/// detected:
///
/// * TCP on port 53 and one other port
/// * UDP on port 53 and one other port
/// * ICMP (by pinging)
///
/// # Limitations
///
/// These tests are performed on one single public IP address
/// and one private IP address. They detect basic leaks but
/// do not guarantee close conformity with the security
/// document.
#[test_function]
pub async fn test_connecting_state(
    _: TestContext,
    rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    let inet_destination = "1.1.1.1:1337".parse().unwrap();
    let lan_destination: SocketAddr = SocketAddr::new(IpAddr::V4(DUMMY_LAN_INTERFACE_IP), 1337);
    let inet_dns = "1.1.1.1:53".parse().unwrap();
    let lan_dns: SocketAddr = SocketAddr::new(IpAddr::V4(DUMMY_LAN_INTERFACE_IP), 53);

    log::info!("Verify tunnel state: disconnected");
    assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

    let relay_settings = RelaySettings::CustomTunnelEndpoint(CustomTunnelEndpoint {
        host: "1.3.3.7".to_owned(),
        config: mullvad_types::ConnectionConfig::Wireguard(unreachable_wireguard_tunnel()),
    });

    set_relay_settings(&mut mullvad_client, relay_settings)
        .await
        .expect("failed to update relay settings");

    mullvad_client
        .connect_tunnel(())
        .await
        .expect("failed to begin connecting");
    let new_state = wait_for_tunnel_state(mullvad_client.clone(), |state| {
        matches!(
            state,
            TunnelState::Connecting { .. } | TunnelState::Error(..)
        )
    })
    .await?;

    assert!(
        matches!(new_state, TunnelState::Connecting { .. }),
        "failed to enter connecting state: {:?}",
        new_state
    );

    //
    // Leak test
    //

    let non_tunnel_interface = rpc
        .get_default_interface()
        .await
        .expect("failed to obtain non-tun interface");

    assert!(
        send_guest_probes(rpc.clone(), non_tunnel_interface.clone(), inet_destination)
            .await?
            .none(),
        "observed unexpected outgoing packets (inet)"
    );
    assert!(
        send_guest_probes(rpc.clone(), non_tunnel_interface.clone(), lan_destination)
            .await?
            .none(),
        "observed unexpected outgoing packets (lan)"
    );
    assert!(
        send_guest_probes(rpc.clone(), non_tunnel_interface.clone(), inet_dns)
            .await?
            .none(),
        "observed unexpected outgoing packets (DNS, inet)"
    );
    assert!(
        send_guest_probes(rpc.clone(), non_tunnel_interface, lan_dns)
            .await?
            .none(),
        "observed unexpected outgoing packets (DNS, lan)"
    );

    assert_tunnel_state!(&mut mullvad_client, TunnelState::Connecting { .. });

    Ok(())
}

/// Try to produce leaks in the error state. Refer to the
/// `test_connecting_state` documentation for details.
#[test_function]
pub async fn test_error_state(
    _: TestContext,
    rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    let inet_destination = "1.1.1.1:1337".parse().unwrap();
    let lan_destination: SocketAddr = SocketAddr::new(IpAddr::V4(DUMMY_LAN_INTERFACE_IP), 1337);
    let inet_dns = "1.1.1.1:53".parse().unwrap();
    let lan_dns: SocketAddr = SocketAddr::new(IpAddr::V4(DUMMY_LAN_INTERFACE_IP), 53);

    log::info!("Verify tunnel state: disconnected");
    assert_tunnel_state!(&mut mullvad_client, TunnelState::Disconnected);

    //
    // Connect to non-existent location
    //

    log::info!("Enter error state");

    let relay_settings = RelaySettings::Normal(RelayConstraints {
        location: Constraint::Only(LocationConstraint::Location(
            GeographicLocationConstraint::Country("xx".to_string()),
        )),
        ..Default::default()
    });

    mullvad_client
        .set_allow_lan(false)
        .await
        .expect("failed to disable LAN sharing");

    set_relay_settings(&mut mullvad_client, relay_settings)
        .await
        .expect("failed to update relay settings");

    let _ = connect_and_wait(&mut mullvad_client).await;
    assert_tunnel_state!(&mut mullvad_client, TunnelState::Error { .. });

    //
    // Leak test
    //

    let default_interface = rpc
        .get_default_interface()
        .await
        .expect("failed to obtain non-tun interface");

    assert!(
        send_guest_probes(rpc.clone(), default_interface.clone(), inet_destination)
            .await?
            .none(),
        "observed unexpected outgoing packets (inet)"
    );
    assert!(
        send_guest_probes(rpc.clone(), default_interface.clone(), lan_destination)
            .await?
            .none(),
        "observed unexpected outgoing packets (lan)"
    );
    assert!(
        send_guest_probes(rpc.clone(), default_interface.clone(), inet_dns)
            .await?
            .none(),
        "observed unexpected outgoing packets (DNS, inet)"
    );
    assert!(
        send_guest_probes(rpc.clone(), default_interface, lan_dns)
            .await?
            .none(),
        "observed unexpected outgoing packets (DNS, lan)"
    );

    Ok(())
}

/// Connect to a single relay and verify that:
/// * Traffic can be sent and received in the tunnel.
///   This is done by pinging a single public IP address
///   and failing if there is no response.
/// * The correct relay is used.
/// * Leaks outside the tunnel are blocked. Refer to the
///   `test_connecting_state` documentation for details.
#[test_function]
pub async fn test_connected_state(
    _: TestContext,
    rpc: ServiceClient,
    mut mullvad_client: ManagementServiceClient,
) -> Result<(), Error> {
    let inet_destination = "1.1.1.1:1337".parse().unwrap();

    //
    // Set relay to use
    //

    log::info!("Select relay");

    let relay_filter = |relay: &Relay| {
        relay.active && matches!(relay.endpoint_data, RelayEndpointData::Wireguard(_))
    };

    let relay = helpers::filter_relays(&mut mullvad_client, relay_filter)
        .await?
        .pop()
        .unwrap();

    let relay_settings = RelaySettings::Normal(RelayConstraints {
        location: helpers::into_constraint(&relay),
        ..Default::default()
    });

    set_relay_settings(&mut mullvad_client, relay_settings)
        .await
        .expect("failed to update relay settings");

    //
    // Connect
    //

    connect_and_wait(&mut mullvad_client).await?;

    let state = get_tunnel_state(&mut mullvad_client).await;

    //
    // Verify that endpoint was selected
    //

    match state {
        TunnelState::Connected {
            endpoint:
                TunnelEndpoint {
                    endpoint:
                        Endpoint {
                            address: SocketAddr::V4(addr),
                            protocol: TransportProtocol::Udp,
                        },
                    // TODO: Consider the type of `relay` / `relay_filter` instead
                    tunnel_type: TunnelType::Wireguard,
                    quantum_resistant: false,
                    proxy: None,
                    obfuscation: None,
                    entry_endpoint: None,
                    tunnel_interface: _,
                },
            ..
        } => {
            assert_eq!(*addr.ip(), relay.ipv4_addr_in);
        }
        actual => panic!("unexpected tunnel state: {:?}", actual),
    }

    //
    // Ping outside of tunnel while connected
    //

    log::info!("Test whether outgoing non-tunnel traffic is blocked");

    let nontun_iface = rpc
        .get_default_interface()
        .await
        .expect("failed to find non-tun interface");

    let detected_probes = send_guest_probes(rpc.clone(), nontun_iface, inet_destination).await?;
    assert!(
        detected_probes.none(),
        "observed unexpected outgoing packets: {detected_probes:?}"
    );

    assert!(
        helpers::using_mullvad_exit(&rpc).await,
        "expected Mullvad exit IP"
    );

    Ok(())
}
