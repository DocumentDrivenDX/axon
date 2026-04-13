import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Axon Admin UI against a PostgreSQL backend.
 *
 * Run with: bunx playwright test --config playwright.e2e.postgres.config.ts
 *
 * Requires a running PostgreSQL instance reachable via AXON_POSTGRES_DSN.
 * The webServer block starts a real `axon serve` instance with postgres
 * storage and no auth so tests exercise the full stack end-to-end.
 *
 * Example DSN: postgresql://postgres:postgres@localhost:5432/postgres
 */
export default defineConfig({
	testDir: './tests/e2e',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
	workers: process.env.CI ? 1 : 1,
	reporter: 'html',

	use: {
		baseURL: 'http://localhost:4171',
		trace: 'on-first-retry',
		screenshot: 'only-on-failure',
	},

	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] },
		},
	],

	/* Start a real axon-server instance backed by PostgreSQL before running tests.
	 * cwd is set to the workspace root so `cargo run` can find the workspace.
	 * AXON_POSTGRES_DSN must be set to a valid PostgreSQL connection string. */
	webServer: {
		command:
			'cargo run -p axon-cli -- serve --no-auth --storage postgres --http-port 4171 --postgres-dsn $AXON_POSTGRES_DSN --ui-dir ui/build',
		url: 'http://localhost:4171/health',
		reuseExistingServer: !process.env.CI,
		timeout: 120000,
		cwd: '..',
		env: {
			AXON_POSTGRES_DSN: process.env.AXON_POSTGRES_DSN ?? '',
		},
	},
});
