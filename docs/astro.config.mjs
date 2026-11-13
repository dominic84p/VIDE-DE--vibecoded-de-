// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightThemeBlack from 'starlight-theme-black';

// https://astro.build/config
export default defineConfig({
	site: 'https://dominic84p.github.io',
	base: '/vide-de/',
	integrations: [
		starlight({
			title: 'VIDE-DE Documentation',
			description: 'A Wayland desktop environment with its own compositor, written in Rust and vibecoded end to end.',
			logo: {
				src: './public/logo.png',
				alt: 'VIDE-DE Logo',
			},
			favicon: '/logo.png',
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/dominic84p/vide-de' },
			],
			sidebar: [
				{ label: 'Home', slug: 'index' },
				{
					label: 'Getting Started',
					items: [
						{ label: 'Overview', slug: 'overview' },
						{ label: 'Installation', slug: 'getting-started/installation' },
					],
				},
				{
					label: 'Architecture',
					items: [
						{ label: 'Overview', slug: 'architecture/overview' },
						{ label: 'Main Event Loop (main.rs)', slug: 'architecture/main-event-loop' },
						{ label: 'Compositor State (state.rs)', slug: 'architecture/state' },
					],
				},
				{
					label: 'Components',
					items: [
						{ label: 'Window Manager (wm.rs)', slug: 'components/window-manager' },
						{ label: 'Input Handling (input.rs)', slug: 'components/input-handling' },
						{ label: 'Rendering (render.rs)', slug: 'components/rendering' },
						{ label: 'UI System (ui.rs + canvas.rs)', slug: 'components/ui' },
						{ label: 'Layer Shell (layers.rs)', slug: 'components/layers' },
						{ label: 'StatusNotifierItem (sni.rs)', slug: 'components/sni' },
						{ label: 'System Tray (tray.rs)', slug: 'components/tray' },
						{ label: 'Virtual Desktops (desktops.rs)', slug: 'components/desktops' },
						{ label: 'Start Menu Search (search.rs)', slug: 'components/search' },
						{ label: 'Screenshots & Snipping (screenshot.rs)', slug: 'components/screenshot' },
						{ label: 'IPC Socket (ipc.rs)', slug: 'components/ipc' },
						{ label: 'App Discovery (apps.rs)', slug: 'components/apps' },
						{ label: 'Icons (icons.rs)', slug: 'components/icons' },
						{ label: 'Cursors (cursor.rs)', slug: 'components/cursor' },
						{ label: 'XWayland (xwayland.rs)', slug: 'components/xwayland' },
						{ label: 'Focus Management (focus.rs)', slug: 'components/focus' },
						{ label: 'Right-Click Menus (menu.rs)', slug: 'components/menu' },
						{ label: 'Pinned Apps (pins.rs)', slug: 'components/pins' },
						{ label: 'Health & Diagnostics (health.rs)', slug: 'components/health' },
					],
				},
				{
					label: 'Configuration',
					items: [
						{ label: 'Settings', slug: 'configuration/settings' },
						{ label: 'Config Library (vide-config)', slug: 'configuration/config-library' },
					],
				},
				{
					label: 'Development',
					items: [
						{ label: 'Contributing', slug: 'development/contributing' },
						{ label: 'Building & Testing', slug: 'development/building' },
						{ label: 'Testing', slug: 'development/testing' },
						{ label: 'Adding Features', slug: 'development/adding-features' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Keybindings', slug: 'reference/keybindings' },
						{ label: 'Settings Reference', slug: 'reference/settings-reference' },
						{ label: 'IPC Commands', slug: 'reference/ipc-commands' },
						{ label: 'CLI Flags', slug: 'reference/cli-flags' },
						{ label: 'Environment Variables', slug: 'reference/env-vars' },
					],
				},
				{
					label: 'Wiki / FAQ',
					items: [
						{ label: 'FAQ', slug: 'wiki/faq' },
						{ label: 'Troubleshooting', slug: 'wiki/troubleshooting' },
						{ label: 'How-To Guides', slug: 'wiki/how-to' },
						{ label: 'Architecture Deep Dive', slug: 'wiki/architecture-deep-dive' },
						{ label: 'Wayland Protocols', slug: 'wiki/wayland-protocols' },
						{ label: 'NVIDIA Guide', slug: 'wiki/nvidia-guide' },
						{ label: 'Multi-Monitor Setup', slug: 'wiki/multi-monitor' },
						{ label: 'Gaming Guide', slug: 'wiki/gaming-guide' },
						{ label: 'Built-in Apps', slug: 'wiki/built-in-apps' },
					],
				},
			],
			plugins: [starlightThemeBlack({})],
			head: [
				{ tag: 'meta', attrs: { name: 'theme-color', content: '#0078d7' } },
			],
		}),
	],
});