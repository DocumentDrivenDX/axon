import { defineConfig, devices } from '@playwright/test';

const baseURL = process.env.AXON_E2E_BASE_URL ?? 'http://localhost:4170';

/**
 * Playwright E2E configuration for the Axon Admin UI.
 *
 * Run with: bun run test:e2e
 *
 * The package scripts delegate to scripts/test-ui-e2e-docker.sh so Playwright
 * always runs inside a pinned browser/runtime container. Set AXON_E2E_BASE_URL
 * to smoke-test an already-running instance without changing the runner.
 *
 * This config targets a real, persistent local service, so it runs serially.
 */
export default defineConfig({
	testDir: './tests/e2e',
	globalSetup: './tests/e2e/cleanup-fixtures.ts',
	globalTeardown: './tests/e2e/cleanup-fixtures.ts',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: 1,
	reporter: 'html',

	use: {
		baseURL,
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
