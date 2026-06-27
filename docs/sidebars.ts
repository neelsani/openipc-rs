import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  mainSidebar: [
    'intro',
    'getting-started',
    'architecture',
    {
      type: 'category',
      label: 'Application Targets',
      items: ['native', 'rust-library', 'web-wasm', 'wasm-sdk', 'desktop-tauri'],
    },
    {
      type: 'category',
      label: 'Protocol And Hardware',
      items: ['video-pipeline', 'adaptive-link', 'realtek-driver'],
    },
    {
      type: 'category',
      label: 'Operations',
      items: ['debugging-metrics', 'publishing', 'ci-cd'],
    },
    {
      type: 'category',
      label: 'Project Notes',
      items: ['references', 'reference-notes', 'roadmap'],
    },
  ],
};

export default sidebars;
