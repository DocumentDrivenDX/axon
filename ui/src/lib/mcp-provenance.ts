/**
 * MCP stdio bridge provenance helpers.
 *
 * The intent detail and audit views need to surface, for an MCP-originated
 * mutation intent, both the agent provenance (already covered by
 * `intent-metadata.ts`) and the stdio transport that the agent connected
 * over. This module synthesises a non-secret command/config preview from
 * the audit origin and intent subject metadata, and applies a redaction
 * pass that masks anything that looks like a credential, token, secret,
 * or other environment-leakage risk.
 */
import type { AuditEntry, MutationIntent } from '$lib/api';
import {
	agentIdentityLabel,
	credentialLabel,
	delegatedByLabel,
	grantVersionLabel,
	isMcpOriginated,
	requesterLabel,
	tenantRoleLabel,
	toolNameLabel,
} from '$lib/intent-metadata';

export type McpStdioConnectionStatus = 'active' | 'idle' | 'stale' | 'unknown';

export type McpStdioEnvEntry = {
	key: string;
	value: string;
	redacted: boolean;
};

export type McpStdioProvenance = {
	surface: string;
	transport: 'stdio' | 'http' | 'unknown';
	command: string[];
	commandText: string;
	configStatus: McpStdioConnectionStatus;
	configStatusLabel: string;
	lastActivityNs: number | null;
	requestId: string | null;
	agentIdentity: string;
	credentialId: string;
	delegatedBy: string;
	tenantRole: string;
	requester: string;
	grantVersion: string;
	toolName: string;
	env: McpStdioEnvEntry[];
};

const SECRET_KEY_PATTERNS = [
	/token/i,
	/secret/i,
	/key$/i,
	/api[_-]?key/i,
	/password/i,
	/credential/i,
	/authorization/i,
	/auth[_-]?header/i,
	/bearer/i,
	/session/i,
];

const SECRET_VALUE_PATTERNS = [/sk-[a-z0-9]/i, /^bearer\s+/i, /^[A-Z0-9_]+_TOKEN=/];

const REDACTED_LABEL = '[redacted]';

function latestAuditEntry(audit: AuditEntry[]): AuditEntry | null {
	let latest: AuditEntry | null = null;
	for (const entry of audit) {
		if (!latest || entry.timestamp_ns > latest.timestamp_ns) {
			latest = entry;
		}
	}
	return latest;
}

function originSurface(audit: AuditEntry[], fallback: string): string {
	const ranked = ['mcp', 'graphql', 'rest', 'cli'];
	for (const target of ranked) {
		for (const entry of audit) {
			if (entry.intent_lineage?.origin?.surface === target) return target;
		}
	}
	for (const entry of audit) {
		const surface = entry.intent_lineage?.origin?.surface;
		if (surface && surface.length > 0 && surface !== 'system') return surface;
	}
	return fallback;
}

function originRequestId(audit: AuditEntry[]): string | null {
	for (const entry of audit) {
		const requestId = entry.intent_lineage?.origin?.request_id;
		if (typeof requestId === 'string' && requestId.length > 0) return requestId;
	}
	return null;
}

function buildLaunchCommand(scope: { tenant: string; database: string }): string[] {
	return ['axon-server', '--mcp-stdio', '--tenant', scope.tenant, '--database', scope.database];
}

// POSIX shell single-quote escaping. Operators copy commandText into a
// terminal, so any tenant/database value containing whitespace or shell
// metacharacters must be quoted such that the shell parses it back as a
// single argument with the original literal value.
function shellQuote(value: string): string {
	if (value === '') return "''";
	if (/^[A-Za-z0-9_./:@%+=-]+$/.test(value)) return value;
	return `'${value.replaceAll("'", "'\\''")}'`;
}

function buildCommandText(command: string[]): string {
	return command.map(shellQuote).join(' ');
}

function classifyConnection(
	lastActivityNs: number | null,
	nowNs: number,
): McpStdioConnectionStatus {
	if (lastActivityNs === null) return 'unknown';
	const elapsedMs = (nowNs - lastActivityNs) / 1_000_000;
	if (elapsedMs <= 5 * 60 * 1000) return 'active';
	if (elapsedMs <= 60 * 60 * 1000) return 'idle';
	return 'stale';
}

function statusLabel(status: McpStdioConnectionStatus, lastActivityNs: number | null): string {
	if (lastActivityNs === null) {
		return 'No recent stdio activity recorded';
	}
	const lastSeen = new Date(Math.floor(lastActivityNs / 1_000_000)).toISOString();
	switch (status) {
		case 'active':
			return `Active (last stdio activity ${lastSeen})`;
		case 'idle':
			return `Idle (last stdio activity ${lastSeen})`;
		case 'stale':
		case 'unknown':
			return `Stale (last stdio activity ${lastSeen})`;
	}
}

export function isSecretEnvKey(key: string): boolean {
	return SECRET_KEY_PATTERNS.some((pattern) => pattern.test(key));
}

export function isSecretEnvValue(value: string): boolean {
	return SECRET_VALUE_PATTERNS.some((pattern) => pattern.test(value));
}

export function redactEnvEntry(entry: { key: string; value: string }): McpStdioEnvEntry {
	const redacted = isSecretEnvKey(entry.key) || isSecretEnvValue(entry.value);
	return {
		key: entry.key,
		value: redacted ? REDACTED_LABEL : entry.value,
		redacted,
	};
}

/**
 * Build a non-secret env preview for the stdio command. The values shown
 * here come from documented operator-facing configuration (tenant/database
 * routing) — never from the live process environment. Anything that
 * matches a credential/secret pattern is redacted before being returned
 * so an operator screenshot cannot leak it downstream.
 */
function buildEnvPreview(
	intent: MutationIntent,
	scope: { tenant: string; database: string },
): McpStdioEnvEntry[] {
	const credential = credentialLabel(intent);
	const entries: Array<{ key: string; value: string }> = [
		{ key: 'AXON_TENANT', value: scope.tenant },
		{ key: 'AXON_DATABASE', value: scope.database },
		{ key: 'AXON_AGENT_ID', value: agentIdentityLabel(intent) },
		{
			key: 'AXON_CREDENTIAL_ID',
			value: credential === '-' ? 'unknown' : credential,
		},
	];
	return entries.map(redactEnvEntry);
}

export type BuildMcpStdioProvenanceArgs = {
	intent: MutationIntent;
	audit: AuditEntry[];
	scope: { tenant: string; database: string };
	now?: number;
};

export function buildMcpStdioProvenance(
	args: BuildMcpStdioProvenanceArgs,
): McpStdioProvenance | null {
	const { intent, audit, scope, now } = args;
	if (!isMcpOriginated(intent)) return null;
	// MCP-originated intents always reach Axon via the agent transport, even
	// when the lifecycle audit row was written by the system surface during
	// preview/approve/commit. Default to the MCP surface for the provenance
	// view and let any explicit non-system audit origin override it.
	const surface = originSurface(audit, 'mcp');
	const transport = surface === 'mcp' ? 'stdio' : surface === 'graphql' ? 'http' : 'unknown';
	const latest = latestAuditEntry(audit);
	const lastActivityNs = latest ? Number(latest.timestamp_ns) : null;
	const nowNs = typeof now === 'number' ? now : Date.now() * 1_000_000;
	const status = classifyConnection(lastActivityNs, nowNs);
	const command = buildLaunchCommand(scope);
	return {
		surface,
		transport,
		command,
		commandText: buildCommandText(command),
		configStatus: status,
		configStatusLabel: statusLabel(status, lastActivityNs),
		lastActivityNs,
		requestId: originRequestId(audit),
		agentIdentity: agentIdentityLabel(intent),
		credentialId: credentialLabel(intent),
		delegatedBy: delegatedByLabel(intent),
		tenantRole: tenantRoleLabel(intent),
		requester: requesterLabel(intent),
		grantVersion: grantVersionLabel(intent),
		toolName: toolNameLabel(intent, audit),
		env: buildEnvPreview(intent, scope),
	};
}
