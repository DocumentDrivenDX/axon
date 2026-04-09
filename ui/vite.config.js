import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [sveltekit()],
	server: {
		proxy: {
			'/audit': 'http://localhost:3000',
			'/collections': 'http://localhost:3000',
			'/entities': 'http://localhost:3000',
			'/health': 'http://localhost:3000',
		},
	},
});
