import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  integrations: [
    starlight({
      title: "Exosuit",
      description:
        "A local-first collaborative cockpit for human and AI development workflows.",
      customCss: ["./src/styles/exosuit.css"],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/wycats/exo2",
        },
      ],
      sidebar: [
        {
          label: "Start Here",
          items: [
            { label: "Exosuit in one page", slug: "start-here" },
            { label: "Philosophy", slug: "start-here/philosophy" },
            { label: "North Star", slug: "start-here/north-star" },
          ],
        },
        {
          label: "Concepts",
          items: [
            {
              label: "Phases, goals, tasks",
              slug: "concepts/phases-goals-tasks",
            },
            { label: "RFCs", slug: "concepts/rfcs" },
            {
              label: "Steering and perception",
              slug: "concepts/steering-and-perception",
            },
            { label: "Local-first state", slug: "concepts/local-first-state" },
          ],
        },
        {
          label: "Guides",
          items: [
            { label: "Getting started", slug: "guides/getting-started" },
            {
              label: "Use Exosuit in an existing repo",
              slug: "guides/use-in-existing-repo",
            },
            { label: "Sidecar setup", slug: "guides/sidecar-setup" },
            { label: "Daily workflow", slug: "guides/daily-workflow" },
            { label: "Run the loop", slug: "guides/run-the-loop" },
            { label: "Start a phase", slug: "guides/start-a-phase" },
            {
              label: "Prepare, execute, review",
              slug: "guides/plan-execute-review",
            },
            { label: "Verify and close", slug: "guides/verify-and-close" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI", slug: "reference/cli" },
            { label: "VS Code extension", slug: "reference/vscode-extension" },
            { label: "Hooks", slug: "reference/hooks" },
            { label: "Sidecars", slug: "reference/sidecars" },
            { label: "State locations", slug: "reference/state-locations" },
          ],
        },
        {
          label: "Design System",
          items: [
            { label: "Style guide", slug: "design-system" },
            { label: "Design Principles", slug: "design-system/principles" },
            { label: "Voice", slug: "design-system/voice" },
            {
              label: "Visual semantics",
              slug: "design-system/visual-semantics",
            },
            {
              label: "Product patterns",
              slug: "design-system/product-patterns",
            },
          ],
        },
        {
          label: "Architecture",
          items: [
            { label: "Overview", slug: "architecture" },
            {
              label: "Validation-based reactivity",
              slug: "architecture/validation-based-reactivity",
            },
            {
              label: "Storage and projections",
              slug: "architecture/storage-and-projections",
            },
            { label: "Sidecars", slug: "architecture/sidecars" },
          ],
        },
      ],
    }),
  ],
});
