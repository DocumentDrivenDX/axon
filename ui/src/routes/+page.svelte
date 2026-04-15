<script lang="ts">
import { goto } from '$app/navigation';
import { base } from '$app/paths';
import { onMount } from 'svelte';

onMount(() => {
	void goto(`${base}/tenants`, { replaceState: true });
});
</script>

<p class="message">Redirecting to tenants...</p>
