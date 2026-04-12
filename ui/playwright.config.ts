import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Axon Admin UI.
 *
 * Run with: bunx playwright test
 *
 * Requires axon-server to be running on localhost:4170 with the built UI:
 *   axon-server --no-auth --storage memory --ui-dir ui/build --http-port 4170
 *
 * All test files use absolute URLs to http://localhost:4170 directly.
 */
export default defineConfig({
	testDir: './tests',
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: process.env.CI ? 1 : undefined,
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
