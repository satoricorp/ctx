import { berkeleyMono } from "./fonts";
import { InstallCommand } from "./install-command";
import { Logo } from "./logo";

const BENEFITS = [
  {
    title: "Shared Local Contexts",
    body: "A single, local context artifact for agents.",
  },
  {
    title: "Editable Markdown",
    body: "Edit your context via `ctx notes`.",
  },
  {
    title: "Agent First",
    body: "Add, use, and query your context using CLI, MCP, and Skills.",
  },
];

const INSTALL_STEPS = [
  {
    title: "Install",
    command: "cargo install --git https://github.com/satoricorp/ctx --bins",
    body: "Installs `ctx`.",
  },
  {
    title: "Set Your OpenAIKey",
    command: "export OPENAI_API_KEY=your_key_here",
    body: "ctx uses OpenAI for extraction and embeddings.",
  },
  {
    title: "Create Context",
    command:
      'ctx init --description "Shared context for Openclaw."',
    body: "Create unlimited, shared, local contexts.",
  },
  {
    title: "Use It",
    command: 'ctx add .\nctx query "what do we already know?"',
    body: "Index any directory and start asking questions.",
  },
];

const USE_CASES = [
  {
    title: "CLI",
    body: "Interact with your context easily through the CLI.",
  },
  {
    title: "Skill",
    body: "`npx skills add satoricorp/ctx` to use the ctx Skill.",
  },
  {
    title: "MCP",
    body: "Run the MCP locally so your agent can query your context.",
  },
];

const CONTRIBUTOR_STEPS = [
  "git clone https://github.com/satoricorp/ctx",
  "cd ctx",
  "cargo install --path . --bins"
];

function SectionHeading({
  eyebrow,
  title,
  body,
}: {
  eyebrow: string;
  title: string;
  body: string;
}) {
  return (
    <div className="flex max-w-4xl flex-col gap-3">
      <p
        className={`${berkeleyMono.className} text-xs uppercase tracking-[0.2em]`}
        style={{ color: "var(--accent-highlight)" }}
      >
        {eyebrow}
      </p>
      <h2 className="text-foreground text-3xl leading-tight font-medium md:text-4xl">
        {title}
      </h2>
      <p className="text-muted-foreground max-w-3xl text-sm leading-7 md:text-base">
        {body}
      </p>
    </div>
  );
}

function CommandCard({
  title,
  command,
  body,
}: {
  title: string;
  command: string;
  body: string;
}) {
  return (
    <div className="bg-background/70 border-foreground/10 flex flex-col gap-5 rounded-2xl border p-7">
      <div className="flex flex-col gap-2">
        <p
          className={`${berkeleyMono.className} text-muted-foreground text-[11px] uppercase tracking-[0.18em]`}
        >
          {title}
        </p>
        <p className="text-muted-foreground text-sm leading-6">{body}</p>
      </div>
      <pre className="bg-muted text-foreground overflow-x-auto rounded-xl px-5 py-4 text-sm leading-6 whitespace-pre">
        <code>{command}</code>
      </pre>
    </div>
  );
}

export default function Home() {
  return (
    <div className="relative isolate min-h-[100vh]">
      <div className="relative z-10 flex min-h-[100vh] flex-col">
        <section className="flex min-h-[100vh] items-center px-4 pb-20 pt-28 md:px-[10%]">
          <div className="mx-auto flex w-full max-w-6xl flex-col gap-8">
            <div className="flex max-w-4xl flex-col items-start gap-6">
              <Logo />
              <p
                className={`${berkeleyMono.className} max-w-prose text-xs uppercase leading-relaxed tracking-[0.2em]`}
                style={{ color: "var(--accent-highlight)" }}
              >
                <span>§</span>
                <span className="ml-2">Spec / 0.2 — Local Context Layer For Agents</span>
              </p>
              <p className="text-foreground max-w-2xl text-lg leading-8 md:text-xl">
                ctx gives you and your agents shared local contexts.
              </p>
              <p className="text-muted-foreground max-w-2xl text-sm leading-7 md:text-base">
                Editable markdown context and file indexing. Query with MCP and Skills.
              </p>
              <div
                className={`${berkeleyMono.className} flex flex-wrap gap-x-4 gap-y-2 text-[11px] uppercase tracking-[0.16em]`}
                style={{ color: "var(--accent-highlight)" }}
              >
                <span>editable markdown</span>
                <span>shared local context</span>
                <span>mcp + skills + cli</span>
              </div>
              <div className="flex w-full max-w-2xl flex-col gap-3">
                <InstallCommand />
                <p className="text-muted-foreground text-sm leading-6">
                  Then set{" "}
                  <code className="text-foreground text-[0.95em]">OPENAI_API_KEY</code>{" "}
                  and run <code className="text-foreground text-[0.95em]">ctx init</code>.
                </p>
              </div>
            </div>
          </div>
        </section>

        <main className="mx-auto flex w-full max-w-6xl flex-col gap-20 px-4 pb-24 md:px-[10%]">
          <section id="benefits" className="flex flex-col gap-8 scroll-mt-28">
            <SectionHeading
              eyebrow="Why ctx"
              title="One source of context."
              body="Editable context via markdown and queryable files in a single artifact."
            />
            <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-3">
              {BENEFITS.map((benefit, index) => (
                <div
                  key={benefit.title}
                  className={`bg-background/70 border-foreground/10 flex h-full min-h-[220px] flex-col gap-4 rounded-2xl border p-7 ${
                    index === 2 ? "md:col-span-2 xl:col-span-1" : ""
                  }`}
                >
                  <h3 className="text-foreground text-lg font-medium">{benefit.title}</h3>
                  <p className="text-muted-foreground text-sm leading-6">{benefit.body}</p>
                </div>
              ))}
            </div>
          </section>

          <section id="install" className="flex flex-col gap-8 scroll-mt-28">
            <SectionHeading
              eyebrow="Install"
              title="Get ctx running in seconds."
              body="Install it, set your key, create a context."
            />
            <div className="flex flex-col gap-5">
              {INSTALL_STEPS.map((step) => (
                <CommandCard
                  key={step.title}
                  title={step.title}
                  command={step.command}
                  body={step.body}
                />
              ))}
            </div>
          </section>

          <section id="use" className="flex flex-col gap-8 scroll-mt-28">
            <SectionHeading
              eyebrow="Use"
              title="Use ctx with any agent."
              body="Use ctx easily through the CLI, MCP, or Skills."
            />
            <div className="grid gap-6 md:grid-cols-3">
              {USE_CASES.map((item) => (
                <div
                  key={item.title}
                  className="bg-background/70 border-foreground/10 flex h-full min-h-[220px] flex-col gap-4 rounded-2xl border p-7"
                >
                  <p
                    className={`${berkeleyMono.className} text-muted-foreground text-[11px] uppercase tracking-[0.18em]`}
                  >
                    {item.title}
                  </p>
                  <p className="text-muted-foreground text-sm leading-6">{item.body}</p>
                </div>
              ))}
            </div>
            <div className="bg-background/70 border-foreground/10 flex flex-col gap-4 rounded-2xl border p-7">
              <p
                className={`${berkeleyMono.className} text-muted-foreground text-[11px] uppercase tracking-[0.18em]`}
              >
                Start MCP
              </p>
              <pre className="bg-muted text-foreground overflow-x-auto rounded-xl px-5 py-4 text-sm leading-6 whitespace-pre">
                <code>ctx mcp --port 8788</code>
              </pre>
            </div>
          </section>

          <section
            id="contribute"
            className="bg-background/70 border-foreground/10 flex flex-col gap-6 rounded-3xl border p-7 scroll-mt-28 md:p-9"
          >
            <SectionHeading
              eyebrow="Contribute"
              title="Contribute to ctx"
              body="Add integrations, improve notes, and grow the community."
            />
            <pre className="bg-muted text-foreground overflow-x-auto rounded-2xl px-4 py-4 text-sm leading-7 whitespace-pre-wrap">
              <code>{CONTRIBUTOR_STEPS.join("\n")}</code>
            </pre>
          </section>
        </main>

        <footer className="mt-auto flex flex-col pb-2 pt-2">
          <div className="bg-foreground/10 mb-2 h-px w-full shrink-0" aria-hidden />
          <div className="flex min-h-9 items-center justify-between gap-4 px-8">
            <a
              href="https://satori.sh"
              className={`${berkeleyMono.className} flex items-center gap-1.5 text-foreground text-xs leading-none tracking-wide transition-colors hover:text-[var(--accent-highlight)]`}
            >
              <span>©</span>
              <span style={{ color: "var(--accent-highlight)" }} aria-hidden>
                ⏺
              </span>
              <span>Satori Engineering Co.</span>
            </a>
          </div>
        </footer>
      </div>
    </div>
  );
}
