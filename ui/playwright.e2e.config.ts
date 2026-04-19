import { defineConfig, devices } from '@playwright/test';

const e2ePort = Number(process.env.AXON_E2E_PORT ?? '4170');
const e2eBaseUrl = `http://localhost:${e2ePort}`;

/**
 * Playwright E2E configuration for the Axon Admin UI against a real server.
 *
 * Run with: bunx playwright test --config playwright.e2e.config.ts
 * If port 4170 is in use, set AXON_E2E_PORT.
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
		baseURL: e2eBaseUrl,
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
		command: `cargo run -p axon-cli -- serve --no-auth --storage memory --ui-dir ui/build --http-port ${e2ePort}`,
		url: `${e2eBaseUrl}/health`,
		reuseExistingServer: !process.env.CI,
		timeout: 120000,
		cwd: '..',
	},
});
