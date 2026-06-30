const GITHUB_URL = 'https://github.com/shushuo/masters'

const features = [
  {
    title: 'Acts on your files',
    body: 'Reads, edits, creates, renames, sorts and dedupes documents in the folders you grant — finishing tasks, not just describing them.',
    tag: 'Files',
  },
  {
    title: 'Grounded in your materials',
    body: 'Ingests your PDFs, slides and notes into a local index, then answers questions with citations back to the exact source.',
    tag: 'Knowledge / RAG',
  },
  {
    title: 'Built for studying',
    body: 'Generates flashcards, runs spaced-repetition review (SM-2), and drafts an adaptive study plan toward your deadline.',
    tag: 'Study',
  },
  {
    title: 'Learns and remembers',
    body: 'File-backed memory and agent-authored skills persist across sessions in Markdown you can read and edit — so it gets better at your work.',
    tag: 'Memory + Skills',
  },
  {
    title: 'Runs your routines',
    body: 'Parameterized recipes plus a cron scheduler turn repeatable chores into one-click or recurring automations, with notification & email digests.',
    tag: 'Recipes + Routines',
  },
  {
    title: 'Extensible via MCP',
    body: 'Add any stdio Model Context Protocol server — Notion, calendars, custom tools — and its tools join the agent, gated and audited like the built-ins.',
    tag: 'Extensions',
  },
]

const masters = [
  {
    title: 'Define masters',
    body: 'Give each master a persona, its own model (any Claude tier, OpenAI or a local model), and a least-privilege toolset. The definition is portable Markdown.',
    tag: 'Personas',
  },
  {
    title: 'Build a team',
    body: 'Group masters under a coordinator. A deterministic router reads each brief and dispatches it to the best-matched master — or the coordinator by default.',
    tag: 'Teams + router',
  },
  {
    title: 'Group chat',
    body: '@-mention one master, several, or the whole team. Replies stream back attributed and live, with bounded multi-round turn-taking so masters answer each other.',
    tag: 'Group chat',
  },
  {
    title: 'Drive coding CLIs',
    body: 'Make a pre-installed Claude Code, Codex, Gemini or OpenCode CLI a first-class master over ACP — its file and permission calls still route through your gate.',
    tag: 'External agents',
  },
]

const comparison = [
  { dim: 'Primary focus', cowork: 'General knowledge work', goose: 'Software engineering', getmasters: 'Studying + personal work' },
  { dim: 'Runs', cowork: 'Cloud-coupled desktop', goose: 'Local', getmasters: 'Local-first' },
  { dim: 'Providers', cowork: 'Anthropic only', goose: '15+ providers', getmasters: 'Claude-first, pluggable' },
  { dim: 'Grounding on your docs', cowork: 'File access', goose: 'Via dev tools', getmasters: 'First-class RAG with citations' },
  { dim: 'Study tools', cowork: '—', goose: '—', getmasters: 'Flashcards · spaced repetition · plans' },
  { dim: 'Multi-agent', cowork: 'Subagents', goose: 'Subagents', getmasters: 'Master teams + group chat' },
  { dim: 'Open source', cowork: 'No', goose: 'Yes', getmasters: 'Yes' },
]

const steps = [
  { n: '01', title: 'Grant a folder', body: 'Point Masters at a folder and choose read or read/write access. Everything it can touch is scoped to what you grant.' },
  { n: '02', title: 'Describe an outcome', body: 'Give it a goal in plain language. Masters plans, reads your materials, and proposes the steps to get there.' },
  { n: '03', title: 'Approve & done', body: 'Writes, deletes and sends pause for your approval with a diff preview. Every action is logged and reversible.' },
]

function Logo({ className = '' }: { className?: string }) {
  return <img src="/logo.svg" alt="Masters panda logo" className={className} />
}

function Pill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center rounded-full border border-accent/30 bg-accent/10 px-3 py-1 text-xs font-medium text-accent-soft">
      {children}
    </span>
  )
}

function FeatureCard({ tag, title, body }: { tag: string; title: string; body: string }) {
  return (
    <div className="card-ring rounded-2xl p-6 transition hover:border-accent/30">
      <span className="text-xs font-medium uppercase tracking-wide text-accent-soft">{tag}</span>
      <h3 className="mt-3 text-lg font-semibold text-cream">{title}</h3>
      <p className="mt-2 text-sm leading-relaxed text-mist">{body}</p>
    </div>
  )
}

export default function App() {
  return (
    <div className="min-h-screen">
      {/* Nav */}
      <header className="sticky top-0 z-30 border-b border-white/5 bg-ink-950/80 backdrop-blur">
        <nav className="mx-auto flex max-w-6xl items-center justify-between px-6 py-4">
          <a href="#top" className="flex items-center gap-2.5">
            <Logo className="h-8 w-8" />
            <span className="text-lg font-bold text-cream">Masters</span>
          </a>
          <div className="hidden items-center gap-8 text-sm text-mist md:flex">
            <a href="#features" className="hover:text-cream">Features</a>
            <a href="#masters" className="hover:text-cream">Masters</a>
            <a href="#how" className="hover:text-cream">How it works</a>
            <a href="#privacy" className="hover:text-cream">Privacy</a>
          </div>
          <a
            href={GITHUB_URL}
            className="rounded-lg border border-white/10 bg-white/5 px-4 py-2 text-sm font-medium text-cream transition hover:bg-white/10"
          >
            GitHub
          </a>
        </nav>
      </header>

      {/* Hero */}
      <main id="top">
        <section className="hero-glow relative overflow-hidden">
          <div className="mx-auto max-w-6xl px-6 pb-20 pt-20 text-center md:pt-28">
            <div className="mb-6 flex justify-center">
              <Logo className="h-24 w-24 drop-shadow-[0_0_30px_rgba(143,179,173,0.3)]" />
            </div>
            <div className="mb-5 flex justify-center">
              <Pill>Local-first · Claude-powered · Open source</Pill>
            </div>
            <h1 className="mx-auto max-w-3xl text-4xl font-extrabold leading-tight tracking-tight text-cream md:text-6xl">
              An agentic desktop companion for{' '}
              <span className="bg-gradient-to-r from-accent-soft to-accent bg-clip-text text-transparent">
                study and work
              </span>
            </h1>
            <p className="mx-auto mt-6 max-w-2xl text-lg text-mist">
              Point Masters at your local folders and it does the work — reading, editing and creating files,
              grounded in your own materials, with a team of masters you assemble and you in control of every
              consequential action.
            </p>
            <div className="mt-9 flex flex-col items-center justify-center gap-3 sm:flex-row">
              <a
                href={GITHUB_URL}
                className="w-full rounded-xl bg-accent px-6 py-3 text-center font-semibold text-ink-950 shadow-lg shadow-accent/20 transition hover:bg-accent-soft sm:w-auto"
              >
                View on GitHub
              </a>
              <a
                href="#masters"
                className="w-full rounded-xl border border-white/10 bg-white/5 px-6 py-3 text-center font-semibold text-cream transition hover:bg-white/10 sm:w-auto"
              >
                Meet the masters
              </a>
            </div>
            <p className="mt-5 text-xs text-mist-faint">
              Open source and in active development — built in the open, decision by decision.
            </p>
          </div>
        </section>

        {/* Features */}
        <section id="features" className="mx-auto max-w-6xl px-6 py-20">
          <div className="mx-auto max-w-2xl text-center">
            <h2 className="text-3xl font-bold text-cream md:text-4xl">Everything a desk needs, locally</h2>
            <p className="mt-4 text-mist">
              Masters's built-in tools are tuned for the two things you do most — learning and getting personal
              work done.
            </p>
          </div>
          <div className="mt-14 grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
            {features.map((f) => (
              <FeatureCard key={f.title} {...f} />
            ))}
          </div>
        </section>

        {/* Masters + Teams */}
        <section id="masters" className="border-y border-white/5 bg-ink-900/40">
          <div className="mx-auto max-w-6xl px-6 py-20">
            <div className="mx-auto max-w-2xl text-center">
              <Pill>Master teams</Pill>
              <h2 className="mt-5 text-3xl font-bold text-cream md:text-4xl">A team of masters, on your machine</h2>
              <p className="mt-4 text-mist">
                Go beyond one assistant. Define specialist masters — each with its own persona, model and tools —
                group them into a team, and chat with the whole room at once. Every master runs gated and audited.
              </p>
            </div>
            <div className="mt-14 grid gap-5 sm:grid-cols-2 lg:grid-cols-4">
              {masters.map((f) => (
                <FeatureCard key={f.title} {...f} />
              ))}
            </div>
          </div>
        </section>

        {/* How it works */}
        <section id="how" className="mx-auto max-w-6xl px-6 py-20">
          <div className="mx-auto max-w-2xl text-center">
            <h2 className="text-3xl font-bold text-cream md:text-4xl">You stay in control</h2>
            <p className="mt-4 text-mist">
              An agent that finishes tasks, with human-in-the-loop approval on anything consequential.
            </p>
          </div>
          <div className="mt-14 grid gap-6 md:grid-cols-3">
            {steps.map((s) => (
              <div key={s.n} className="relative rounded-2xl border border-white/5 bg-ink-900/60 p-7">
                <span className="text-4xl font-extrabold text-accent/30">{s.n}</span>
                <h3 className="mt-3 text-lg font-semibold text-cream">{s.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-mist">{s.body}</p>
              </div>
            ))}
          </div>
        </section>

        {/* Comparison */}
        <section id="compare" className="border-y border-white/5 bg-ink-900/40">
          <div className="mx-auto max-w-6xl px-6 py-20">
            <div className="mx-auto max-w-2xl text-center">
              <h2 className="text-3xl font-bold text-cream md:text-4xl">Where Masters fits</h2>
              <p className="mt-4 text-mist">
                Inspired by Claude Cowork's philosophy and Goose's architecture — focused on the individual.
              </p>
            </div>
            <div className="mt-12 overflow-x-auto">
              <table className="w-full min-w-[640px] border-separate border-spacing-0 text-left text-sm">
                <thead>
                  <tr className="text-mist">
                    <th className="px-4 py-3 font-medium"></th>
                    <th className="px-4 py-3 font-medium">Claude Cowork</th>
                    <th className="px-4 py-3 font-medium">Goose</th>
                    <th className="rounded-t-xl bg-accent/10 px-4 py-3 font-semibold text-accent-soft">Masters</th>
                  </tr>
                </thead>
                <tbody>
                  {comparison.map((row, i) => (
                    <tr key={row.dim} className="text-cream/80">
                      <td className="border-t border-white/5 px-4 py-4 font-medium text-cream">{row.dim}</td>
                      <td className="border-t border-white/5 px-4 py-4 text-mist">{row.cowork}</td>
                      <td className="border-t border-white/5 px-4 py-4 text-mist">{row.goose}</td>
                      <td
                        className={`border-t border-white/5 bg-accent/10 px-4 py-4 font-medium text-cream ${
                          i === comparison.length - 1 ? 'rounded-b-xl' : ''
                        }`}
                      >
                        {row.getmasters}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </section>

        {/* Privacy */}
        <section id="privacy" className="mx-auto max-w-6xl px-6 py-20">
          <div className="grid items-center gap-10 md:grid-cols-2">
            <div>
              <Pill>Private by construction</Pill>
              <h2 className="mt-5 text-3xl font-bold text-cream md:text-4xl">Your files never leave your machine</h2>
              <p className="mt-4 text-mist">
                Documents, embeddings, sessions and the audit log all live locally in a single SQLite database.
                Only the model context you can see is sent to your chosen provider — and a fully-local mode keeps
                even that on-device.
              </p>
            </div>
            <ul className="space-y-4">
              {[
                'Folder-scoped permissions — tools can only act where you allow',
                'Per-action approvals for writes, deletes and network calls',
                'Diff preview before edits · soft-delete · revert last action',
                'Append-only audit log of every tool call — built-in, MCP or coding CLI',
                'API keys stored in your OS keychain, never in plaintext',
              ].map((item) => (
                <li key={item} className="flex items-start gap-3 text-cream/80">
                  <span className="mt-1 inline-flex h-5 w-5 flex-none items-center justify-center rounded-full bg-accent/20 text-xs text-accent-soft">
                    ✓
                  </span>
                  <span className="text-sm">{item}</span>
                </li>
              ))}
            </ul>
          </div>
        </section>

        {/* CTA */}
        <section className="border-t border-white/5 bg-ink-900/40">
          <div className="mx-auto max-w-6xl px-6 py-24 text-center">
            <h2 className="mx-auto max-w-2xl text-3xl font-bold text-cream md:text-4xl">
              Built in the open, designed for one person — deeply
            </h2>
            <p className="mx-auto mt-4 max-w-xl text-mist">
              Follow the design docs and roadmap, or star the project to track progress toward the first release.
            </p>
            <div className="mt-8 flex justify-center">
              <a
                href={GITHUB_URL}
                className="rounded-xl bg-accent px-7 py-3 font-semibold text-ink-950 shadow-lg shadow-accent/20 transition hover:bg-accent-soft"
              >
                Explore the repository
              </a>
            </div>
          </div>
        </section>
      </main>

      {/* Footer */}
      <footer className="border-t border-white/5">
        <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 py-8 text-sm text-mist-faint sm:flex-row">
          <div className="flex items-center gap-2.5">
            <Logo className="h-6 w-6" />
            <span className="font-semibold text-cream/80">Masters</span>
            <span>· local-first study &amp; work agent</span>
          </div>
          <div className="flex items-center gap-6">
            <a href={GITHUB_URL} className="hover:text-cream">GitHub</a>
            <span>Apache-2.0</span>
          </div>
        </div>
      </footer>
    </div>
  )
}
