<script>
// Axon Admin UI layout
</script>

<nav>
	<a href="/">Collections</a>
	<a href="/schemas">Schemas</a>
	<a href="/audit">Audit Log</a>
</nav>

<main>
	<slot />
</main>

<style>
	nav {
		display: flex;
		gap: 1rem;
		padding: 1rem;
		background: #1a1a2e;
		color: white;
	}
	nav a {
		color: #e0e0e0;
		text-decoration: none;
	}
	nav a:hover {
		color: white;
	}
	main {
		padding: 1rem;
		max-width: 1200px;
		margin: 0 auto;
	}
</style>
