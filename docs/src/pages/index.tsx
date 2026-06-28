import Heading from "@theme/Heading";
import Layout from "@theme/Layout";
import Link from "@docusaurus/Link";

const paths = [
  {
    title: "Run the station",
    body: "Start the browser app, build the Tauri desktop app, or open the hosted build.",
    to: "/docs/getting-started",
    action: "Start Here",
  },
  {
    title: "Use the Rust crates",
    body: "Pull in the packet pipeline, Realtek helpers, WFB/FEC logic, and adaptive-link code.",
    to: "/docs/rust-library",
    action: "Rust Examples",
  },
  {
    title: "Use the WASM SDK",
    body: "Request a WebUSB adapter, receive Annex-B frames, and feed WebCodecs from a web app.",
    to: "/docs/wasm-sdk",
    action: "WASM Examples",
  },
  {
    title: "Check the driver",
    body: "See what is implemented, what came from the reference projects, and what still needs hardware testing.",
    to: "/docs/realtek-driver",
    action: "Driver Notes",
  },
];

const pipeline = [
  "USB bulk IN",
  "Realtek aggregate",
  "WFB decrypt/FEC",
  "RTP + payload taps",
  "Annex-B frames",
  "WebCodecs/player",
];

const references = [
  ["OpenIPC Docs", "https://docs.openipc.org/"],
  ["devourer", "https://github.com/OpenIPC/devourer"],
  ["aviateur", "https://github.com/OpenIPC/aviateur"],
  ["nusb-webusb", "https://docs.rs/nusb-webusb/latest/nusb/"],
];

function PipelineGraphic(): JSX.Element {
  return (
    <div className="pipelineGraphic" aria-label="OpenIPC receive pipeline">
      {pipeline.map((step, index) => (
        <div className="pipelineStep" key={step}>
          <span className="pipelineIndex">
            {String(index + 1).padStart(2, "0")}
          </span>
          <span>{step}</span>
        </div>
      ))}
    </div>
  );
}

export default function Home(): JSX.Element {
  return (
    <Layout
      title="Rust OpenIPC ground-station stack"
      description="Rust crates, WebUSB SDK, and station app for OpenIPC video"
    >
      <main className="homePage">
        <section className="heroShell">
          <div className="container heroGrid">
            <div className="heroCopy">
              <div className="statusLine">
                <span>Rust core</span>
                <span>Native desktop</span>
                <span>WASM + WebUSB</span>
              </div>
              <Heading as="h1" className="homeTitle">
                openipc-rs
              </Heading>
              <p className="homeSubtitle">
                Rust code for receiving OpenIPC video through Realtek USB WiFi
                adapters, with a browser/WebUSB app, a Tauri desktop app, and
                reusable crates for custom ground stations.
              </p>
              <div className="heroActions">
                <Link
                  className="button button--primary button--lg"
                  to="/docs/getting-started"
                >
                  Build And Run
                </Link>
                <a
                  className="button button--secondary button--lg"
                  href="https://station.openipc-rs.neels.dev"
                >
                  Open Station
                </a>
                <Link
                  className="button button--secondary button--lg"
                  to="/docs/architecture"
                >
                  Architecture
                </Link>
              </div>
            </div>
            <div className="heroPanel">
              <div className="panelHeader">
                <span>receiver stack</span>
                <span>shared logic first</span>
              </div>
              <PipelineGraphic />
              <div className="signalReadout">
                <div>
                  <strong>Targets</strong>
                  <span>Linux, macOS, Windows, browser</span>
                </div>
                <div>
                  <strong>Output</strong>
                  <span>H.264/H.265 Annex-B frames</span>
                </div>
              </div>
            </div>
          </div>
        </section>

        <section className="container pathSection">
          <div className="sectionKicker">Choose A Path</div>
          <Heading as="h2">
            Run the app, use the libraries, or inspect the radio path.
          </Heading>
          <div className="pathGrid">
            {paths.map((path) => (
              <article className="pathCard" key={path.title}>
                <Heading as="h3">{path.title}</Heading>
                <p>{path.body}</p>
                <Link to={path.to}>{path.action}</Link>
              </article>
            ))}
          </div>
        </section>

        <section className="homeBand">
          <div className="container bandGrid">
            <div>
              <div className="sectionKicker">What Is Shared</div>
              <Heading as="h2">The packet path lives in Rust.</Heading>
              <p>
                Native and web builds share Realtek RX parsing, WFB session and
                data handling, Reed-Solomon recovery, RTP depacketization,
                generic raw payload taps, adaptive-link feedback generation, and
                TX packet construction.
              </p>
            </div>
            <pre className="codePreview">
              {`let packets = parse_rx_aggregate(&transfer)?;
for packet in packets {
    let events = pipeline.push_80211_frame(packet.data)?;
    // RtpPacket and VideoFrame events expose the layer you need.
}`}
            </pre>
          </div>
        </section>

        <section className="container referenceStrip">
          <div>
            <div className="sectionKicker">Reference Material</div>
            <Heading as="h2">
              Grounded in the projects that already work.
            </Heading>
          </div>
          <div className="referenceLinks">
            {references.map(([label, href]) => (
              <a href={href} key={href}>
                {label}
              </a>
            ))}
            <Link to="/docs/references">All References</Link>
          </div>
        </section>
      </main>
    </Layout>
  );
}
