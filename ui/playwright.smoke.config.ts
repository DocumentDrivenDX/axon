import { defineConfig, devices } from '@playwright/test';

/**
 * One-off smoke config: point Playwright at an already-running HTTPS
 * axon-server (self-signed cert) and execute only smoke-restructure.spec.ts.
 *
 * Run with:
 *   bunx playwright test --config playwright.smoke.config.ts
 */
export default defineConfig({
	testDir: './tests/e2e',
	testMatch: 'smoke-restructure.spec.ts',
	fullyParallel: false,
	workers: 1,
	reporter: [['list']],

	use: {
		baseURL: 'https://localhost:4170',
		ignoreHTTPSErrors: true,
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure',
	},

	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] },
		},
	],
});
