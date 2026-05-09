#!/usr/bin/env bun
/**
 * check-story-coverage.ts
 *
 * Verifies that every FEAT-031 user story (US-113..US-119) has at least one
 * Playwright test or describe block tagged with `@US-NNN` in ui/tests/e2e/.
 *
 * Usage:
 *   bun scripts/check-story-coverage.ts
 *
 * Exits 0 if all stories are covered, non-zero otherwise.
 */

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const REQUIRED_STORIES = [113, 114, 115, 116, 117, 118, 119] as const;

const E2E_DIR = join(import.meta.dir, '..', 'tests', 'e2e');

// Collect all spec files
const specFiles = readdirSync(E2E_DIR).filter((f) => f.endsWith('.spec.ts'));

// Map: US number -> array of spec filenames that contain a tag
const coverage = new Map<number, string[]>();
for (const story of REQUIRED_STORIES) {
	coverage.set(story, []);
}

const tagPattern = /@US-(\d+)/g;

for (const file of specFiles) {
	const content = readFileSync(join(E2E_DIR, file), 'utf-8');
	const found = new Set<number>();
	let match: RegExpExecArray | null;
	tagPattern.lastIndex = 0;
	while ((match = tagPattern.exec(content)) !== null) {
		const num = parseInt(match[1], 10);
		if (REQUIRED_STORIES.includes(num as (typeof REQUIRED_STORIES)[number])) {
			found.add(num);
		}
	}
	for (const num of found) {
		coverage.get(num)?.push(file);
	}
}

// Report
console.log('Story coverage report (FEAT-031 / FEAT-015 / FEAT-016 / FEAT-029 / FEAT-030):');
console.log('');

let allCovered = true;
for (const story of REQUIRED_STORIES) {
	const specs = coverage.get(story) ?? [];
	if (specs.length === 0) {
		console.error(`  MISSING  US-${story}  — no tagged test found`);
		allCovered = false;
	} else {
		console.log(`  COVERED  US-${story}  — ${specs.join(', ')}`);
	}
}

console.log('');

if (!allCovered) {
	const uncovered = REQUIRED_STORIES.filter((s) => (coverage.get(s) ?? []).length === 0);
	const label = uncovered.map((s) => `US-${s}`).join(', ');
	console.error(`uncovered: ${label}`);
	process.exit(1);
}

console.log('All FEAT-031 stories have at least one live-server workflow test tag.');
