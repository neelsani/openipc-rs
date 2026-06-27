import Heading from '@theme/Heading';
import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';

const paths = [
  {
    title: 'Run The Station',
    body: 'Build the browser/WebUSB app or the Tauri desktop shell from the same React UI.',
    to: '/docs/getting-started',
    action: 'Start Here',
  },
  {
    title: 'Use The Rust Crates',
    body: 'Embed the receiver pipeline, Realtek parser, WFB/FEC logic, and adaptive-link helpers.',
    to: '/docs/rust-library',
    action: 'Rust Examples',
  },
  {
    title: 'Use The WASM SDK',
    body: 'Request a WebUSB adapter, initialize monitor mode, receive Annex-B frames, and feed WebCodecs.',
    to: '/docs/wasm-sdk',
    action: 'WASM Examples',
  },
  {
    title: 'Understand The Driver',
    body: 'Follow the Realtek USB bring-up boundary, shared HAL, and hardware validation status.',
    to: '/docs/realtek-driver',
    action: 'Driver Notes',
  },
];

const pipeline = [
  'USB bulk IN',
  'Realtek aggregate',
  'WFB decrypt/FEC',
  'RTP depacketize',
  'Annex-B frames',
  'WebCodecs/player',
];

const references = [
  ['OpenIPC Docs', 'https://docs.openipc.org/'],
  ['devourer', 'https://github.com/OpenIPC/devourer'],
  ['aviateur', 'https://github.com/OpenIPC/aviateur'],
  ['nusb-webusb', 'https://docs.rs/nusb-webusb/latest/nusb/'],
];

function PipelineGraphic(): JSX.Element {
  return (
    <div className="pipelineGraphic" aria-label="OpenIPC receive pipeline">
      {pipeline.map((step, index) => (
        <div className="pipelineStep" key={step}>
          <span className="pipelineIndex">{String(index + 1).padStart(2, '0')}</span>
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
      description="Rust receiver building blocks for OpenIPC FPV ground-station applications"
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
                A clean Rust implementation of the OpenIPC receive path, built
                for native ground stations, browser WebUSB apps, and reusable
                protocol libraries.
              </p>
              <div className="heroActions">
                <Link className="button button--primary button--lg" to="/docs/getting-started">
                  Build And Run
                </Link>
                <Link className="button button--secondary button--lg" to="/docs/architecture">
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
          <Heading as="h2">Build an app, inspect the protocol, or work on hardware.</Heading>
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
                adaptive-link feedback generation, and TX packet construction.
              </p>
            </div>
            <pre className="codePreview">
{`let packets = parse_rx_aggregate(&transfer)?;
for packet in packets {
    let events = pipeline.push_80211_frame(packet.data)?;
    // VideoFrame events carry Annex-B H.264/H.265.
}`}
            </pre>
          </div>
        </section>

        <section className="container referenceStrip">
          <div>
            <div className="sectionKicker">Reference Material</div>
            <Heading as="h2">Grounded in the projects that already work.</Heading>
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
