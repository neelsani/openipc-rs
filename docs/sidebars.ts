import type { SidebarsConfig } from "@docusaurus/plugin-content-docs";

const sidebars: SidebarsConfig = {
  mainSidebar: [
    "intro",
    "getting-started",
    "nebulus",
    "architecture",
    {
      type: "category",
      label: "Libraries And Apps",
      items: [
        "crates",
        "rust-library",
        "native",
        "native-video",
        "web-wasm",
        "wasm-sdk",
        "desktop-tauri",
      ],
    },
    {
      type: "category",
      label: "Protocol And Hardware",
      items: [
        "video-pipeline",
        "low-latency",
        "adaptive-link",
        "realtek-driver",
        "devourer-parity",
      ],
    },
    {
      type: "category",
      label: "Operations",
      items: ["debugging-metrics", "publishing", "ci-cd"],
    },
    {
      type: "category",
      label: "Project Notes",
      items: ["references", "reference-notes", "roadmap"],
    },
  ],
};

export default sidebars;
