import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Axon Admin UI.
 *
 * Run with: bunx playwright test
 *
 * Requires axon to be installed and running on localhost:4170:
 *   axon server install
 *   axon server start
 *
 * This config targets a real, persistent local service, so it runs serially.
 */
export default defineConfig({
	testDir: './tests/e2e',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: 1,
	reporter: 'html',

	use: {
		baseURL: 'http://localhost:4170',
		trace: 'on-first-retry',
		screenshot: 'only-on-failure',
	},

	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] },
		},
	],
});
