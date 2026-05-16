import { berkeleyMono } from "./fonts";
import { InstallCommand } from "./install-command";
import { Logo } from "./logo";

const USE_STEPS = [
  {
    title: "Install",
    command:
      "brew tap satoricorp/tap && brew install satoricorp/tap/ctx\nexport OPENAI_API_KEY=your_key_here",
    body: "Install `ctx` and set your OpenAI key.",
  },
  {
    title: "Initialize",
    command:
      'ctx init\nnpx skills add satoricorp/ctx',
    body: "Create a context and add the ctx Skill.",
  },
  {
    title: "Remember",
    command:
      'ctx remember "Openclaw uses ctx for shared local context."\nctx query "what does Openclaw use ctx for?"\n\nAsk your agent: use the ctx Skill to remember that Openclaw uses ctx for shared local context.',
    body: "Save a fact with the CLI or the ctx Skill, then ask about it.",
  },
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
      <pre className="bg-foreground/[0.04] text-foreground overflow-x-auto rounded-xl px-5 py-4 text-sm leading-6 whitespace-pre dark:bg-white/[0.06]">
        <code>{command}</code>
      </pre>
    </div>
  );
}

function NoiseGradientPanel() {
  return (
    <div className="noise-gradient-panel" aria-hidden>
      <div className="noise-gradient-layer noise-gradient-layer-a" />
      <div className="noise-gradient-layer noise-gradient-layer-b" />
      <div className="noise-gradient-layer noise-gradient-layer-c" />
    </div>
  );
}

export default function Home() {
  return (
    <div className="site-shell relative isolate min-h-[100vh]">
      <div className="relative z-10 flex min-h-[100vh] flex-col">
        <section className="px-4 pb-12 pt-24 md:px-8">
          <div className="noise-panel-frame border-foreground/10 relative mx-auto flex min-h-[34rem] w-full max-w-7xl items-center overflow-hidden rounded-[2rem] border px-6 py-16 md:min-h-[40rem] md:px-[8%]">
            <NoiseGradientPanel />
            <div className="relative z-10 flex w-full max-w-6xl flex-col gap-8">
              <div className="flex max-w-4xl flex-col items-start gap-6">
                <Logo />
                <div className="flex w-full max-w-2xl flex-col items-start gap-3">
                  <p
                    className={`${berkeleyMono.className} max-w-prose text-xs uppercase leading-tight tracking-[0.2em]`}
                    style={{ color: "var(--accent-highlight)" }}
                  >
                    <span>§</span>
                    <span className="ml-2">Spec / 0.2</span>
                  </p>
                  <div className="flex max-w-2xl flex-col gap-1">
                    <p className="text-foreground m-0 text-lg md:text-xl">
                      Shared local context for agents.
                    </p>
                    <p className="text-muted-foreground m-0 text-sm md:text-base">
                      One editable, queryable source of context for agents and
                      humans.
                    </p>
                    <div
                      className={`${berkeleyMono.className} mb-2 flex flex-wrap gap-x-4 gap-y-2 text-[11px] uppercase tracking-[0.16em]`}
                      style={{ color: "var(--accent-highlight)" }}
                    >
                      <span>editable markdown</span>
                      <span>shared local context</span>
                      <span>mcp + skills + cli</span>
                    </div>
                  </div>
                  <InstallCommand />
                </div>
              </div>
            </div>
          </div>
        </section>

        <main className="mx-auto flex w-full max-w-6xl flex-col gap-20 px-4 pb-20 md:px-[10%]">
          <section id="use" className="flex flex-col gap-8 scroll-mt-28">
            <SectionHeading
              eyebrow="Use"
              title="Get ctx running in seconds."
              body="Install it, set your key, create a context."
            />
            <div className="flex flex-col gap-5">
              {USE_STEPS.map((step) => (
                <CommandCard
                  key={step.title}
                  title={step.title}
                  command={step.command}
                  body={step.body}
                />
              ))}
            </div>
          </section>

        </main>

        <section className="px-4 pb-24 md:px-8">
          <div className="noise-panel-frame border-foreground/10 relative mx-auto flex min-h-[34rem] w-full max-w-7xl items-center overflow-hidden rounded-[2rem] border px-6 py-16 md:min-h-[40rem] md:px-[8%]">
            <NoiseGradientPanel />
            <div className="relative z-10 flex w-full max-w-6xl flex-col gap-5">
              <SectionHeading
                eyebrow="Try ctx now"
                title="Install ctx."
                body="Run the install command in your terminal."
              />
              <InstallCommand />
            </div>
          </div>
        </section>

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
