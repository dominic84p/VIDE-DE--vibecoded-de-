---
title: 'VIDE-DE Documentation'
description: 'A Wayland desktop environment with its own compositor, written in Rust and vibecoded end to end.'
---

<div class="hero">
	<h1>VIDE-DE Documentation</h1>
	<p>A Wayland compositor + desktop shell built on <strong>Smithay 0.7</strong>, written entirely in Rust through AI-assisted "vibecoding". No human-written code accepted.</p>
	<div class="hero-buttons">
		<a href="/getting-started/installation/" class="btn btn-primary">Get Started →</a>
		<a href="https://github.com/dominic84p/vide-de" class="btn btn-secondary" target="_blank" rel="noopener">GitHub Repository</a>
	</div>
</div>

<div class="ai-only">
	<span>🤖</span> AI-Only Project — See <a href="/development/contributing/" style="color: inherit; text-decoration: underline;">Contributing Rules</a>
</div>

## Features at a Glance

<div class="feature-grid">
	<div class="feature-card">
		<div class="feature-icon">🖥️</div>
		<h3 class="feature-title">Dual Backend Architecture</h3>
		<p class="feature-desc">Runs nested (for development via winit) or on real hardware (TTY/DRM/KMS/libseat). Both backends share the same rendering pipeline.</p>
	</div>
	<div class="feature-card">
		<div class="feature-icon">🪟</div>
		<h3 class="feature-title">Windows 10-Style Desktop</h3>
		<p class="feature-desc">Taskbar, Start menu with search, system tray, virtual desktops (10 workspaces), Task View panel, window snapping.</p>
	</div>
	<div class="feature-card">
		<div class="feature-icon">🎨</div>
		<h3 class="feature-title">CPU-Painted UI</h3>
		<p class="feature-desc">All chrome (taskbar, menus, titlebars) rendered on CPU via custom canvas, cached by key. Debug builds slow; use release.</p>
	</div>
	<div class="feature-card">
		<div class="feature-icon">🎮</div>
		<h3 class="feature-title">XWayland + Gaming</h3>
		<p class="feature-desc">Full GPU acceleration via linux-dmabuf. Relative pointer + pointer constraints for games. X11 windows managed natively.</p>
	</div>
	<div class="feature-card">
		<div class="feature-icon">🔧</div>
		<h3 class="feature-title">Built-in Apps</h3>
		<p class="feature-desc">Settings, PowerSlop (terminal), File Exploder, Task Mangler, Snip & Sketchy, About — all registered as .desktop entries.</p>
	</div>
	<div class="feature-card">
		<div class="feature-icon">📦</div>
		<h3 class="feature-title">StatusNotifierItem</h3>
		<p class="feature-desc">App tray icons via D-Bus. We host the SNI watcher and provide the host. Volume/brightness flyouts built in.</p>
	</div>
</div>

## Quick Links

| Section | Description |
|---------|-------------|
| [Installation](/getting-started/installation/) | Build and run VIDE-DE |
| [Architecture](/architecture/overview/) | How the compositor works |
| [Components](/components/window-manager/) | Deep dives into each component |
| [Configuration](/configuration/settings/) | Settings and customization |
| [Keybindings](/reference/keybindings/) | Complete keyboard shortcut reference |
| [Wiki/FAQ](/wiki/faq/) | Common questions and troubleshooting |
| [Changelog](/changelog/) | Automatic git history |
| [Contributors](/contributors/) | Who built this |
| [Git Browser](/git-browser/) | Browse all source files with syntax highlighting |

## The Rules

This project follows strict contribution rules — see [CONTRIBUTING.md](https://github.com/dominic84p/vide-de/blob/main/CONTRIBUTING.md):

<div class="rule-note">
<strong>🤖 NO HUMAN SLOP</strong> — Only AI-written code accepted. If a human typed it, it doesn't ship.
</div>
<div class="rule-note">
<strong>📏 NO AI UNDER 20B PARAMETERS</strong> — Small models may watch, but they may not touch.
</div>
<div class="rule-note">
<strong>⚡ AI FOR EVERYTHING</strong> — Code, commits, PRs, reviews. If you run git commit with your own hands, see Rule 1.
</div>
<div class="rule-note">
<strong>🎯 40K TOKEN MINIMUM</strong> — Low-effort PRs get the <code>minimal</code> label. Show token count in PR description.
</div>
<div class="rule-note">
<strong>🔒 SECURITY BY CLAUDE ONLY</strong> — Human-found bugs don't count. See <a href="https://github.com/dominic84p/vide-de/blob/main/SECURITY.md">SECURITY.md</a>.
</div>

## Get Started

```bash
# Build (nested mode)
cargo build --release --workspace

# Run nested (inside current desktop)
cargo run --release -- --nested

# Run on real hardware (from TTY)
dbus-run-session -- ./target/release/vibe-comp --tty
```

See [Installation Guide](/getting-started/installation/) for detailed instructions.