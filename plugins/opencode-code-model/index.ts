import { constants } from "node:fs"
import { access, readFile } from "node:fs/promises"
import { delimiter, dirname, isAbsolute, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { Plugin, Skill } from "@opencode/plugin"

type Reindex = "never" | "if-missing" | "if-stale" | "always"

const REINDEX: readonly Reindex[] = ["never", "if-missing", "if-stale", "always"]

interface Options {
  /** Path to the `model-mcp` binary. Defaults to $MODEL_MCP_BIN, PATH, then local builds. */
  readonly binary?: string
  /** Path to `model.json`. Defaults to $MODEL_MCP_MODEL, then <project>/model.json. */
  readonly model?: string
  /** Source root passed to `model-mcp --root`. `false` disables source rebuild. Defaults to the project. */
  readonly root?: string | false
  /** Indexer policy passed to `model-mcp --reindex`. Defaults to `never`. */
  readonly reindex?: Reindex
  /** Expose MCP tools through Code Mode. Defaults to true. */
  readonly codemode?: boolean
}

const SKILLS = [
  { id: "model-context", file: "skills/model-context/SKILL.md" },
  { id: "model-impact", file: "skills/model-impact/SKILL.md" },
  { id: "model-architecture", file: "skills/model-architecture/SKILL.md" },
  { id: "graph-exploration", file: "skills/graph-exploration/SKILL.md" },
] as const

function fromProject(path: string, project: string): string {
  return isAbsolute(path) ? path : resolve(project, path)
}

async function isExecutable(path: string): Promise<boolean> {
  try {
    await access(path, constants.X_OK)
    return true
  } catch {
    return false
  }
}

async function findBinary(project: string, here: string, options: Options): Promise<string | undefined> {
  const name = process.platform === "win32" ? "model-mcp.exe" : "model-mcp"
  const candidates: string[] = []

  const configured = options.binary ?? process.env.MODEL_MCP_BIN
  if (configured) candidates.push(fromProject(configured, project))
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    if (dir) candidates.push(join(dir, name))
  }
  for (const base of [project, here, dirname(here)]) {
    candidates.push(join(base, "target", "release", name))
    candidates.push(join(base, "target", "debug", name))
  }

  for (const candidate of candidates) {
    if (await isExecutable(candidate)) return candidate
  }
  return undefined
}

function fields(content: string): Record<string, string> {
  const match = /^---\r?\n([\s\S]*?)\r?\n---/.exec(content)
  const result: Record<string, string> = {}
  if (!match) return result
  for (const line of match[1].split(/\r?\n/)) {
    const colon = line.indexOf(":")
    if (colon < 1) continue
    result[line.slice(0, colon).trim()] = line.slice(colon + 1).trim().replace(/^['"]|['"]$/g, "")
  }
  return result
}

function body(content: string): string {
  const match = /^---\r?\n[\s\S]*?\r?\n---\r?\n?/.exec(content)
  return match ? content.slice(match[0].length) : content
}

export default Plugin.define({
  id: "code-model",
  async setup(ctx) {
    const here = dirname(fileURLToPath(import.meta.url))
    const project = ctx.location.project?.canonical ?? ctx.location.directory
    const options = (ctx.options ?? {}) as Options

    const binary = await findBinary(project, here, options)
    const model = fromProject(
      options.model ?? process.env.MODEL_MCP_MODEL ?? join(project, "model.json"),
      project,
    )
    const configuredRoot =
      options.root !== undefined ? options.root : (process.env.MODEL_MCP_ROOT ?? project)
    const root = configuredRoot === false ? undefined : fromProject(configuredRoot, project)
    const reindex: Reindex = REINDEX.includes(options.reindex as Reindex)
      ? (options.reindex as Reindex)
      : "never"
    const args = ["--model", model]
    if (root) args.push("--root", root)
    args.push("--reindex", reindex)

    if (binary) console.info(`[code-model] model MCP: ${binary} ${args.join(" ")}`)
    else console.warn("[code-model] model-mcp not found; build it with: cargo build -p model-mcp --release")

    // An explicitly configured `model` server wins: do not override the user's setup.
    const mcp = await ctx.mcp.transform((editor) => {
      if (editor.get("model")) return
      editor.set("model", {
        type: "local",
        command: [binary ?? "model-mcp", ...args],
        cwd: project,
        disabled: !binary,
        codemode: options.codemode !== false,
      })
    })

    const skills = await Promise.all(
      SKILLS.map(async ({ id, file }) => {
        const path = join(here, file)
        const content = await readFile(path, "utf8")
        const meta = fields(content)
        return { id, path, content, name: meta.name ?? id, description: meta.description ?? "" }
      }),
    )
    const registeredSkills = await ctx.skill.transform((editor) => {
      for (const skill of skills) {
        editor.add({
          id: Skill.ID.make(skill.id),
          name: Skill.Name.make(skill.name),
          description: skill.description,
          path: Skill.Info.fields.path.make(skill.path),
          content: skill.content,
        })
      }
    })

    // Plugins cannot register agents, so the impact-reviewer agent is also a command.
    // The project's copy wins when present, otherwise the bundled one is used.
    let instructions = body(await readFile(join(here, "agents", "impact-reviewer.md"), "utf8"))
    try {
      instructions = body(await readFile(join(project, ".opencode", "agents", "impact-reviewer.md"), "utf8"))
    } catch {
      // No project copy: keep the bundled instructions.
    }
    const command = await ctx.command.transform((editor) => {
      editor.add({
        name: "model-impact-review",
        description: "Review a change's impact with the in-memory code model (callers, hotspots, tests)",
        execute: async ({ sessionID, prompt, delivery }) => {
          await ctx.session.prompt({
            ...prompt,
            sessionID,
            text: `${instructions.trim()}\n\n---\n\n${prompt.text}`,
            delivery,
          })
        },
      })
    })

    return async () => {
      await Promise.all([mcp.dispose(), registeredSkills.dispose(), command.dispose()])
    }
  },
})
