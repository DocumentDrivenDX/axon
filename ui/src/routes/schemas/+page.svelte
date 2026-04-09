<script>
/** US-041: Manage schemas visually */
// Schema list, view formatted JSON, edit and save, inline validation
const schemas = [];
let selectedSchema = null;
let editMode = false;
let editJson = '';
let validationError = null;

function selectSchema(schema) {
	selectedSchema = schema;
	editJson = JSON.stringify(schema, null, 2);
	editMode = false;
	validationError = null;
}

function validateJson() {
	try {
		JSON.parse(editJson);
		validationError = null;
		return true;
	} catch (e) {
		validationError = e.message;
		return false;
	}
}
</script>

<h1>Schema Management</h1>

<div class="layout">
	<div class="list">
		<h2>Collections</h2>
		{#if schemas.length === 0}
			<p>No schemas registered. Create a collection to add a schema.</p>
		{/if}
		{#each schemas as schema}
			<button on:click={() => selectSchema(schema)}>
				{schema.collection}
			</button>
		{/each}
	</div>

	<div class="detail">
		{#if selectedSchema}
			<h2>{selectedSchema.collection} Schema</h2>
			{#if editMode}
				<textarea bind:value={editJson} on:input={validateJson}></textarea>
				{#if validationError}
					<p class="error">{validationError}</p>
				{/if}
				<button on:click={() => editMode = false}>Cancel</button>
				<button disabled={!!validationError}>Save</button>
			{:else}
				<pre>{JSON.stringify(selectedSchema, null, 2)}</pre>
				<button on:click={() => editMode = true}>Edit</button>
			{/if}
		{:else}
			<p>Select a collection to view its schema.</p>
		{/if}
	</div>
</div>

<style>
	.layout { display: flex; gap: 1rem; }
	.list { width: 200px; }
	.detail { flex: 1; }
	textarea { width: 100%; height: 300px; font-family: monospace; }
	pre { background: #f5f5f5; padding: 1rem; overflow-x: auto; }
	.error { color: red; font-size: 0.9em; }
</style>
