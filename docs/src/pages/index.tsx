import Heading from "@theme/Heading";
import Layout from "@theme/Layout";
import Link from "@docusaurus/Link";
import useBaseUrl from "@docusaurus/useBaseUrl";

const guides = [
  {
    number: "01",
    title: "Run Nebulus",
    body: "Connect a supported Realtek adapter from the native app, Android, or a WebUSB browser.",
    to: "/docs/nebulus",
    action: "Ground station guide",
  },
  {
    number: "02",
    title: "Build an application",
    body: "Use the shared Rust receiver, routing, RTP, video, and adaptive-link APIs in your own ground station.",
    to: "/docs/rust-library",
    action: "Rust library guide",
  },
  {
    number: "03",
    title: "Understand the radio path",
    body: "Follow bytes from USB aggregates through 802.11, WFB recovery, payload routes, and platform decoding.",
    to: "/docs/architecture",
    action: "Architecture overview",
  },
];

const pipeline = [
  "Realtek USB",
  "802.11 monitor frames",
  "WFB decrypt + FEC",
  "Payload routes",
  "RTP depacketization",
  "Native video",
];

const crates = [
  ["openipc-core", "WFB, FEC, RTP, routes, and adaptive link"],
  ["openipc-rtl88xx", "User-space Realtek USB WiFi driver"],
  ["openipc-video", "Low-latency platform video decoders"],
  ["openipc-web", "WASM bindings and WebUSB receiver API"],
];

function ReceiverPreview({ logo }: { logo: string }): JSX.Element {
  return (
    <div className="receiverPreview" aria-label="Nebulus receiver preview">
      <div className="previewHeader">
        <span className="previewBrand">
          <span className="previewLive" /> Nebulus
        </span>
        <span>RECEIVING</span>
      </div>
      <div className="previewVideo">
        <img src={logo} alt="" />
        <div className="previewReticle" aria-hidden="true" />
        <div className="previewTelemetry">
          <span>1080p</span>
          <span>60 fps</span>
          <span>18.4 Mbps</span>
          <span>7.2 ms</span>
          <span>-61 dBm</span>
        </div>
      </div>
      <div className="previewFooter">
        <span>LINK 94</span>
        <div className="previewBars" aria-hidden="true">
          <i />
          <i />
          <i />
          <i />
          <i />
        </div>
        <span>LOSS 0.0%</span>
      </div>
    </div>
  );
}

export default function Home(): JSX.Element {
  const logo = useBaseUrl("/img/logo.svg");

  return (
    <Layout
      title="Nebulus OpenIPC ground station"
      description="Nebulus is a low-latency OpenIPC ground station built on reusable Rust receiver, driver, routing, and video crates."
    >
      <main className="homePage">
        <section className="nebHero">
          <div className="container nebHeroInner">
            <div className="nebHeroCopy">
              <div className="nebEyebrow">
                <img src={logo} alt="" />
                <span>Built on openipc-rs</span>
              </div>
              <Heading as="h1">Nebulus</Heading>
              <p className="nebHeroLead">
                A low-latency OpenIPC ground station for desktop, Android, and
                the browser, backed by a reusable Rust receiver stack.
              </p>
              <div className="nebActions">
                <a
                  className="button button--primary button--lg"
                  href="https://nebulus.openipc-rs.neels.dev"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  Launch Web App
                </a>
                <a
                  className="button nebButtonGhost button--lg"
                  href="https://github.com/neelsani/openipc-rs/releases/latest"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  Download Nebulus
                </a>
                <Link className="nebTextLink" to="/docs/getting-started">
                  Read the docs <span aria-hidden="true">→</span>
                </Link>
              </div>
              <div className="nebTargets" aria-label="Supported targets">
                <span>macOS</span>
                <span>Windows</span>
                <span>Linux</span>
                <span>Android</span>
                <span>WebUSB</span>
              </div>
            </div>
            <ReceiverPreview logo={logo} />
          </div>
        </section>

        <section className="container nebStart">
          <div className="nebSectionHeading">
            <span>Start here</span>
            <Heading as="h2">One stack, two ways to use it.</Heading>
            <p>
              Run Nebulus as a complete ground station or use the same Rust
              components to build something purpose-specific.
            </p>
          </div>
          <div className="nebGuideGrid">
            {guides.map((guide) => (
              <Link className="nebGuide" to={guide.to} key={guide.title}>
                <span className="nebGuideNumber">{guide.number}</span>
                <Heading as="h3">{guide.title}</Heading>
                <p>{guide.body}</p>
                <strong>
                  {guide.action} <span aria-hidden="true">→</span>
                </strong>
              </Link>
            ))}
          </div>
        </section>

        <section className="nebStack">
          <div className="container">
            <div className="nebSectionHeading nebSectionHeading--wide">
              <span>Receive pipeline</span>
              <Heading as="h2">The complete packet path stays in Rust.</Heading>
              <p>
                Native and browser builds share protocol behavior. Only USB
                access, video decode, presentation, and OS integrations vary by
                platform.
              </p>
            </div>
            <div className="nebPipeline">
              {pipeline.map((step, index) => (
                <div className="nebPipelineStep" key={step}>
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  <strong>{step}</strong>
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="container nebLibraries">
          <div className="nebLibrariesIntro">
            <span>For developers</span>
            <Heading as="h2">Use only the layer you need.</Heading>
            <p>
              The public crates separate protocol processing, hardware access,
              platform decoding, and browser bindings without tying applications
              to the Nebulus UI.
            </p>
            <Link to="/docs/crates">Explore all crates →</Link>
          </div>
          <div className="nebCrateList">
            {crates.map(([name, description]) => (
              <Link to="/docs/crates" className="nebCrate" key={name}>
                <code>{name}</code>
                <span>{description}</span>
                <b aria-hidden="true">→</b>
              </Link>
            ))}
          </div>
        </section>
      </main>
    </Layout>
  );
}
