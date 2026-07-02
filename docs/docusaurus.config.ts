import type { Config } from "@docusaurus/types";
import type * as Preset from "@docusaurus/preset-classic";

const REPO_URL = "https://github.com/neelsani/openipc-rs";

function clean(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function buildInfo() {
  const enabled = clean(process.env.OPENIPC_BUILD_METADATA) === "true";
  const commit = enabled
    ? (clean(process.env.OPENIPC_GIT_COMMIT)?.slice(0, 40) ?? null)
    : null;
  const shortCommit =
    commit === null
      ? null
      : (clean(process.env.OPENIPC_GIT_SHORT_COMMIT) ?? commit.slice(0, 7));

  return {
    commit,
    shortCommit,
    tag: commit === null ? null : clean(process.env.OPENIPC_GIT_TAG),
    dirty: false,
    repoUrl: REPO_URL,
    commitUrl: commit ? `${REPO_URL}/commit/${commit}` : REPO_URL,
  };
}

const config: Config = {
  title: "openipc-rs",
  tagline: "Nebulus ground station and Rust libraries for OpenIPC FPV",
  favicon: "img/logo.svg",

  url: process.env.DOCUSAURUS_URL ?? "https://openipc-rs.neels.dev",
  baseUrl: process.env.DOCUSAURUS_BASE_URL ?? "/",

  organizationName: "neelsani",
  projectName: "openipc-rs",
  customFields: {
    buildInfo: buildInfo(),
  },

  onBrokenLinks: "throw",
  markdown: {
    mermaid: true,
    hooks: {
      onBrokenMarkdownLinks: "warn",
    },
  },
  trailingSlash: false,

  i18n: {
    defaultLocale: "en",
    locales: ["en"],
    localeConfigs: {
      en: {
        label: "English",
        direction: "ltr",
        htmlLang: "en-US",
      },
    },
  },

  presets: [
    [
      "classic",
      {
        docs: {
          sidebarPath: "./sidebars.ts",
          routeBasePath: "docs",
          editUrl: "https://github.com/neelsani/openipc-rs/edit/master/docs/",
        },
        blog: false,
        theme: {
          customCss: "./src/css/custom.css",
        },
      } satisfies Preset.Options,
    ],
  ],
  themes: ["@docusaurus/theme-mermaid"],

  themeConfig: {
    image: "img/logo.svg",
    navbar: {
      title: "openipc-rs",
      logo: {
        alt: "openipc-rs",
        src: "img/logo.svg",
      },
      items: [
        {
          type: "docSidebar",
          sidebarId: "mainSidebar",
          position: "left",
          label: "Docs",
        },
        {
          type: "custom-buildInfo",
          position: "right",
        },
        {
          href: "https://github.com/neelsani/openipc-rs",
          label: "GitHub",
          position: "right",
        },
        {
          href: "https://nebulus.openipc-rs.neels.dev",
          label: "Open Nebulus",
          position: "right",
        },
        {
          type: "localeDropdown",
          position: "right",
        },
      ],
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Build",
          items: [
            {
              label: "Getting Started",
              to: "/docs/getting-started",
            },
            {
              label: "Native",
              to: "/docs/native",
            },
            {
              label: "Web/WASM",
              to: "/docs/web-wasm",
            },
          ],
        },
        {
          title: "Internals",
          items: [
            {
              label: "Architecture",
              to: "/docs/architecture",
            },
            {
              label: "Realtek Driver",
              to: "/docs/realtek-driver",
            },
            {
              label: "Adaptive Link",
              to: "/docs/adaptive-link",
            },
          ],
        },
        {
          title: "Project",
          items: [
            {
              label: "GitHub",
              href: "https://github.com/neelsani/openipc-rs",
            },
            {
              label: "Roadmap",
              to: "/docs/roadmap",
            },
            {
              label: "Nebulus",
              href: "https://nebulus.openipc-rs.neels.dev",
            },
            {
              label: "Legacy Station",
              href: "https://station.openipc-rs.neels.dev",
            },
          ],
        },
      ],
      copyright: `Copyright ${new Date().getFullYear()} openipc-rs contributors. Released under the MIT License.`,
    },
    prism: {
      additionalLanguages: ["rust", "toml", "bash"],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
