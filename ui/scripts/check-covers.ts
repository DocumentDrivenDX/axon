#!/usr/bin/env bun
/**
 * check-covers.ts
 *
 * Scans ui/tests/e2e/*.spec.ts and sdk/typescript/test/*.test.ts for
 * `@covers US-NNN-ACm` canonical citation tags and reports which
 * acceptance criteria have at least one citing test.
 *
 * Usage:
 *   bun scripts/check-covers.ts
 *
 * Exits 0 if every required AC has at least one citation, non-zero otherwise.
 */

import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

// ACs expected to be COVERED by UI/SDK tests per the bead scope.
const REQUIRED_COVERS = [
	'US-113-AC1',
	'US-113-AC2',
	'US-113-AC3',
	'US-113-AC4',
	'US-113-AC5',
	'US-113-AC6',
	'US-114-AC1',
	'US-114-AC2',
	'US-114-AC3',
	'US-114-AC4',
	'US-115-AC1',
	'US-115-AC2',
	'US-115-AC3',
	'US-115-AC4',
	'US-116-AC1',
	'US-116-AC2',
	'US-116-AC3',
	'US-116-AC4',
	'US-117-AC1',
	'US-117-AC2',
	'US-117-AC3',
	'US-117-AC4',
	'US-117-AC5',
	'US-118-AC1',
	'US-118-AC2',
	'US-118-AC3',
	'US-118-AC4',
	'US-119-AC1',
	'US-119-AC2',
	'US-119-AC3',
	'US-119-AC4',
	'US-105-AC6',
	'US-106-AC4',
	'US-107-AC6',
] as const;

type CoveredAC = (typeof REQUIRED_COVERS)[number];

const REPO_ROOT = join(import.meta.dir, '..', '..');
const E2E_DIR = join(import.meta.dir, '..', 'tests', 'e2e');
const SDK_TEST_DIR = join(REPO_ROOT, 'sdk', 'typescript', 'test');

// Map: AC id -> array of "file:testTitle" citations
const citations = new Map<CoveredAC, string[]>();
for (const ac of REQUIRED_COVERS) {
	citations.set(ac, []);
}

const coversPattern = /@covers (US-\d+-AC\d+)/g;

function scanDir(dir: string, glob: string) {
	let files: string[];
	try {
		files = readdirSync(dir).filter((f) => f.endsWith(glob));
	} catch {
		return; // directory may not exist in some environments
	}
	for (const file of files) {
		const absPath = join(dir, file);
		const relPath = relative(REPO_ROOT, absPath);
		const content = readFileSync(absPath, 'utf-8');
		// Extract each test/it title line (single-quoted or double-quoted string after test( or it()
		const titlePattern = /(?:test|it)\s*\(\s*(['"])(.*?)\1/g;
		let titleMatch: RegExpExecArray | null;
		titlePattern.lastIndex = 0;
		while ((titleMatch = titlePattern.exec(content)) !== null) {
			const title = titleMatch[2];
			coversPattern.lastIndex = 0;
			let coverMatch: RegExpExecArray | null;
			while ((coverMatch = coversPattern.exec(title)) !== null) {
				const ac = coverMatch[1] as CoveredAC;
				if (citations.has(ac)) {
					citations.get(ac)!.push(`${relPath}: "${title}"`);
				}
			}
		}
	}
}

scanDir(E2E_DIR, '.spec.ts');
scanDir(SDK_TEST_DIR, '.test.ts');

// Report
console.log('@covers citation report (UI E2E + SDK):');
console.log('');

let allCovered = true;
for (const ac of REQUIRED_COVERS) {
	const cites = citations.get(ac) ?? [];
	if (cites.length === 0) {
		console.error(`  MISSING  ${ac}  — no @covers citation found`);
		allCovered = false;
	} else {
		console.log(`  COVERED  ${ac}  — ${cites.length} citation(s)`);
		for (const c of cites) {
			console.log(`             ${c}`);
		}
	}
}

console.log('');

if (!allCovered) {
	const uncovered = REQUIRED_COVERS.filter((ac) => (citations.get(ac) ?? []).length === 0);
	console.error(`uncovered: ${uncovered.join(', ')}`);
	process.exit(1);
}

console.log('All required AC citations present.');
