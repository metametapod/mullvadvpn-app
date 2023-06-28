import { expect, test } from '@playwright/test';
import { Locator, Page } from 'playwright';
import { RoutePath } from '../../../../src/renderer/lib/routes';
import { TestUtils } from '../../utils';

import { startInstalledApp } from '../installed-utils';

// This test expects the daemon to be logged out and the provided account to have five registered
// devices..
// Env parameters:
//   `ACCOUNT_NUMBER`: Account number to use when logging in

let page: Page;
let util: TestUtils;

test.beforeAll(async () => {
  ({ page, util } = await startInstalledApp());
});

test.afterAll(async () => {
  await page.close();
});

test('App should show too many devices', async () => {
  expect(await util.currentRoute()).toEqual(RoutePath.login);

  const loginInput = getInput(page);
  await loginInput.type(process.env.ACCOUNT_NUMBER!);

  expect(await util.waitForNavigation(() => {
    loginInput.press('Enter');
  })).toEqual(RoutePath.tooManyDevices);

  const loginButton = page.getByText('Continue with login');

  await expect(page.getByTestId('title')).toHaveText('Too many devices');
  await expect(loginButton).toBeDisabled();
  await page.getByLabel(/^Remove device named/).first().click();
  await page.getByText('Yes, log out device').click();

  await expect(loginButton).toBeEnabled();

  // Trigger transition: too-many-devices -> login -> main
  expect(await util.waitForNavigation(() => {
    loginButton.click();
  })).toEqual(RoutePath.login);

  // Note: `util.waitForNavigation` won't return the navigation event when
  // transitioning from login -> main, so we need to observe the state of the
  // app after the entire transition chain has finished.
  await util.waitForNoTransition();
  await expect(page.getByTestId(RoutePath.main)).toBeVisible();
});

function getInput(page: Page): Locator {
  return page.getByPlaceholder('0000 0000 0000 0000');
}
