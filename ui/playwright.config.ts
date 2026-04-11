import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Axon Admin UI.
 *
 * Run with: bunx playwright test
 *
 * Requires the Axon server to be running on localhost:3000 so the Vite dev
 * server proxy can forward API requests.
 */
export default defineConfig({
	testDir: './tests',
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: process.env.CI ? 1 : undefined,
	reporter: 'html',

	use: {
		baseURL: 'http://localhost:5173',
		trace: 'on-first-retry',
		screenshot: 'only-on-failure',
	},

	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] },
		},
	],

	/* Start the SvelteKit dev server before running tests. */
	webServer: {
		command: 'bun run dev',
		url: 'http://localhost:5173',
		reuseExistingServer: !process.env.CI,
	},
});
