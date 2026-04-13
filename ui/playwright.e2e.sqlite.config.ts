import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright E2E configuration for the Axon Admin UI against a SQLite backend.
 *
 * Run with: bunx playwright test --config playwright.e2e.sqlite.config.ts
 *
 * The webServer block starts a real `axon serve` instance with SQLite storage
 * persisted to SQLITE_DB_PATH so tests can verify disk persistence directly.
 *
 * Note: the database is removed before starting so each run begins clean.
 * persistence.spec.ts imports SQLITE_DB_PATH to query the file directly via sqlite3.
 */

export const SQLITE_DB_PATH = '/tmp/axon-e2e-sqlite.db';

export default defineConfig({
	testDir: './tests/e2e',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 1 : 0,
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

	/* Start a fresh axon-server instance backed by SQLite.
	 * The database file is deleted first so each run starts clean.
	 * cwd is set to the workspace root so `cargo run` can find the workspace. */
	webServer: {
		command: `rm -f ${SQLITE_DB_PATH} && cargo run -p axon-cli -- serve --no-auth --storage sqlite --sqlite-path ${SQLITE_DB_PATH} --ui-dir ui/build`,
		url: 'http://localhost:4170/health',
		reuseExistingServer: !process.env.CI,
		timeout: 120000,
		cwd: '..',
	},
});
