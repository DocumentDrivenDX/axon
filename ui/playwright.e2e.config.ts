import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Axon Admin UI against a real server.
 *
 * Run with: bunx playwright test --config playwright.e2e.config.ts
 *
 * The webServer block starts a real `axon serve` instance with in-memory
 * storage and no auth so tests exercise the full stack end-to-end.
 */
export default defineConfig({
	testDir: './tests/e2e',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: process.env.CI ? 1 : 1,
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

	/* Start a real axon-server instance before running E2E tests.
	 * cwd is set to the workspace root so `cargo run` can find the workspace. */
	webServer: {
		command: 'cargo run -p axon-cli -- serve --no-auth --storage memory --ui-dir ui/build',
		url: 'http://localhost:4170/health',
		reuseExistingServer: !process.env.CI,
		timeout: 120000,
		cwd: '..',
	},
});
