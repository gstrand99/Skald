import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import gruvbox from "starlight-theme-gruvbox";

export default defineConfig({
	site: "https://tryskald.dev",
	integrations: [
		starlight({
			title: "Skald",
			description:
				"Linux-first, local-first dictation: record, transcribe, clipboard, and safe paste.",
			social: [
				{
					icon: "github",
					label: "GitHub",
					href: "https://github.com/gstrand/skald",
				},
			],
			plugins: [gruvbox()],
			sidebar: [
				{
					label: "Getting started",
					items: [
						{ label: "Introduction", slug: "index" },
						{ label: "Install", slug: "install" },
						{ label: "Setup wizard", slug: "setup" },
					],
				},
				{
					label: "Configuration",
					items: [
						{ label: "Overview", slug: "configuration" },
						{ label: "daemon", slug: "configuration/daemon" },
						{ label: "paths", slug: "configuration/paths" },
						{ label: "audio", slug: "configuration/audio" },
						{ label: "asr", slug: "configuration/asr" },
						{ label: "vocabulary", slug: "configuration/vocabulary" },
						{ label: "cleanup", slug: "configuration/cleanup" },
						{ label: "diagnostics", slug: "configuration/diagnostics" },
						{ label: "secrets", slug: "configuration/secrets" },
						{ label: "injection", slug: "configuration/injection" },
						{ label: "notifications", slug: "configuration/notifications" },
						{ label: "privacy", slug: "configuration/privacy" },
						{ label: "voice_commands", slug: "configuration/voice-commands" },
						{ label: "preview", slug: "configuration/preview" },
						{ label: "overlay", slug: "configuration/overlay" },
						{ label: "Related files", slug: "configuration/related-files" },
					],
				},
				{
					label: "Using Skald",
					items: [
						{ label: "CLI reference", slug: "cli" },
						{ label: "Service & shortcuts", slug: "service" },
						{ label: "Troubleshooting", slug: "troubleshooting" },
					],
				},
				{
					label: "Linux",
					items: [
						{ label: "Releases", slug: "linux/releases" },
						{ label: "Desktop matrix", slug: "linux/desktop-matrix" },
						{ label: "Benchmark results", slug: "linux/benchmarks" },
					],
				},
			],
			customCss: ["./src/styles/custom.css"],
		}),
	],
});
