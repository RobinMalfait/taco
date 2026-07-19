## Taco

> It's a wrapper around your commands!

### Eh? What are you talking about...

Let's imagine you have 2 projects, and you want to run `tests` in each project.

1. `Project A`, is a Laravel PHP project, so you want to use `phpunit` or `pest`.
2. `Project b`, is a JavaScript project, so you want to use `vitest` or `npm run test`.

I don't want to remember all of that... Let's fix it.

```sh
cd ~/projects/php_project_a
taco add test -- phpunit

cd ~/projects/js_project_b
taco add test -- npm run test
```

So what happened here? We created aliases!

This is what the config looks like in `~/.config/taco/taco.json`

```json
{
  "projects": {
    "/Users/robin/projects/php_project_a": {
      "test": "phpunit"
    },
    "/Users/robin/projects/js_project_b": {
      "test": "npm run test"
    }
  }
}
```

From now on, I can just write `taco test` regardless of the project I am in, and it will execute the corresponding command. This is awesome because I work
in a lot of different projects, and a lot of them are not even mine. It would be stupid to change all the scripts for each project just because I like `npm run tdd` instead of `npm run test:watch` as a script name.

#### Inheritance

Scripts inherit scripts from **parent** directories. This allows you to set the `npm run test` only once in a shared folder. In my case, I did this in a `~/github.com/tailwindlabs` folder. Commands defined in deeper directories win over the ones from parent directories.

This is how I use it personally:

```json
{
  "projects": {
    "/Users/robin/github.com": {
      "tdd": "./node_modules/.bin/vitest --watch",
      "test": "./node_modules/.bin/vitest"
    },
    "/Users/robin/github.com/tailwindlabs": {
      "dev": "next dev"
    },
    "/Users/robin/github.com/tailwindlabs/tailwindcss": {
      "build": "bun run swcify",
      "watch": "bun run swcify --watch"
    },
    "/Users/robin/github.com/tailwindlabs/headlessui": {
      "vue": "yarn workspace @headlessui/vue",
      "react": "yarn workspace @headlessui/react"
    }
  }
}
```

#### Sharing commands – `.taco.json`

Everything above lives in your personal config, but commands can also be committed alongside a project. Any directory can contain a `.taco.json` file:

```json
{
  "commands": {
    "build": "cargo build --release",
    "test": "cargo nextest run"
  }
}
```

(The commands are scoped under a `commands` key so the format can grow later without breaking existing files.)

Everyone on the team gets these commands right after cloning. They follow the same inheritance rules as your own commands: they are visible in subdirectories, and deeper directories win over parents. Within the same directory your personal commands and aliases win over the repo-local ones, so `taco add` always lets you override a shared command for yourself.

`taco print` and `taco which` show these commands with the `.taco.json` file they come from. You don't have to write the file by hand either: `taco add --local`, `taco edit --local` and `taco rm --local` manage the `.taco.json` of the current directory (removing the last command also removes the file), and `taco config --local` opens it in your editor. Without `--local`, those commands only ever touch your personal config.

Note: as with a `Makefile` or npm scripts, running a command from a freshly cloned repository executes whatever the author put there — taco never runs anything you didn't explicitly invoke, but do glance at `taco print` in repositories you don't trust yet.

#### Which command wins

When you run `taco test`, the command can be defined in several places at once. Taco looks for it in this order — the first match wins:

1. Your own commands for the current directory (`projects` in your config)
2. Commands inherited via aliases attached to the current directory (`aliases` in your config)
3. The `.taco.json` in the current directory
4. The same three steps for the parent directory
5. ... and so on, all the way up to the root of the filesystem

Read top to bottom, the first match wins:

```
~/projects/app                 ← the current directory
├─ 1. your commands            (config)
├─ 2. your aliases             (config)
├─ 3. .taco.json
│
└─ ~/projects                  ← its parent
   ├─ 4. your commands         (config)
   ├─ 5. your aliases          (config)
   ├─ 6. .taco.json
   │
   └─ /                        ← … and so on, up to the filesystem root
      ├─ 7. your commands      (config)
      ├─ 8. your aliases       (config)
      ├─ 9. .taco.json
      │
      └─ 10. taco's builtin subcommands
```

In short: deeper directories win over parents, and within the same directory your personal config wins over aliases, which win over the shared `.taco.json`. When multiple aliases are attached to the same directory, the one added last wins.

The builtin subcommands come after all of the above — your commands always win over them (see below). Use `taco which {name}` to see where a command comes from, including the definitions it shadows.

#### Your commands win

Your commands always win over taco's builtin subcommands, so a new taco version can never break your existing commands. If you define a command named `config`, then `taco config` runs _your_ command.

The builtins stay reachable through the `taco` namespace:

```sh
taco config        # your command
taco taco config   # the builtin
```

`taco` itself is the only reserved name (`taco add taco` refuses), which means `taco taco {subcommand}` unambiguously refers to the builtin — use that form in scripts. Shadowing a builtin is perfectly fine, but taco makes it visible: `taco add` prints a note when you create one, `taco print` tags them, and `taco doctor` lists them.

---

## Requirements

- This is a Rust project and the binaries are not published
  anywhere. This means that you need to have Rust/Cargo installed.

## Installation

```sh
cargo build --release
```

This will create a `taco` binary at `./target/release/taco`.

### Optional quality of life improvements

I am using `zshrc`, and I make sure to export the `./target/release/` folder so
that I can automatically run the `taco` binary.

```sh
PATH=./target/release:$PATH
```

In addition, I also have this `PATH`, so that I can run `taco` from
anywhere on my system.

```sh
PATH=/path-to-taco-project/target/release:$PATH
```

---

### Usage

#### Add – `taco add {name}`

```sh
taco add ls
```

This will open your default editor (`$VISUAL`, falling back to `$EDITOR`) to input the command. Lines with `#` at the start will be ignored, and commands can span multiple lines.

If the alias already exists, you will be asked to confirm before it is overwritten.

#### Add – `taco add {name} -- {command}`

```sh
taco add ls -- ls -lah
# Aliased "ls" to "ls -lah" in /Users/robin
```

Performs the same action as above, but without opening an editor. This is useful
if it's a simple one-liner command.

Both forms accept `--local` to store the command in the `.taco.json` of the current directory instead of your personal config, so it can be committed and shared. If your personal config defines the same command in the same directory, taco notes that yours wins.

#### Edit – `taco edit {name}`

```sh
taco edit ls
```

This will open your default editor (`$VISUAL`, falling back to `$EDITOR`) to edit the command. The current command will be pre-filled. Lines with `#` at the start will be ignored.

With `--local`, this edits the command in the `.taco.json` of the current directory instead.

#### Execute – `taco {name} -- {passthrough arguments}`

```sh
taco ls
# total 680
# total 680
# drwxr-x---+ 59 robin  staff   1.8K Dec  1 21:06 .
# drwxr-xr-x   5 root   admin   160B Nov 15 18:46 ..
# -rw-r--r--@  1 robin  staff    18K Dec  1 19:38 .DS_Store
# drwx------+ 56 robin  staff   1.8K Nov 29 18:43 .Trash
# ...
```

Additional arguments are passed through to the underlying command. Use `--` if you want to pass flags:

```sh
taco test -- --watch
# Runs: ./node_modules/.bin/vitest --watch
```

Or if you want to look at the command that is going to be executed use the `--print` (or `-p`) flag.

```sh
taco ls --print
# ls -lah
```

The command runs through your shell (`$SHELL`), and taco exits with the same exit code as the command itself.

If you make a typo, taco suggests the closest command instead of printing the full list:

```sh
taco tets
# Command `tets` does not exist.
#
# Did you mean taco test?
```

The same suggestions apply to `taco edit`, `taco which`, `taco rm` and `taco unalias`.

#### Which – `taco which {name}`

Shows the command that would run, and where it is defined. If the command is defined in multiple places (a parent directory, or an alias), the shadowed definitions are listed too, closest one first.

```sh
taco which test
# taco test
#   cargo nextest run --release
#
# Defined in rust (via alias in /Users/robin/github.com/RobinMalfait/taco)
#
# Shadowed definitions:
#   /Users/robin/github.com
#     ./node_modules/.bin/vitest
```

#### Alias – `taco alias {name}`

Next to inheriting commands from parent directories, a project can also inherit the commands from any other "project" in your config. A project doesn't have to be a path — it can be just a name, which makes it easy to define reusable presets like `vitest`, `prettier`, `vite`, `tailwind`, `next`, ...

```sh
cd ~/projects/project-a
taco alias vitest
# Added "vitest" capabilities in /Users/robin/projects/project-a
```

This is stored in the `aliases` section of the config:

```json
{
  "aliases": {
    "/Users/robin/projects/project-a": ["vitest"],
    "/Users/robin/projects/project-b": ["vitest"]
  },
  "projects": {
    "vitest": {
      "tdd": "vitest --hideSkippedTests",
      "test": "vitest run"
    }
  }
}
```

Now `taco test` and `taco tdd` work in both projects (and their subdirectories), without repeating the commands per project. A project can have multiple aliases, and commands defined in the project itself win over commands inherited via aliases.

Note: named projects like `vitest` only exist in the config file — since `taco add` always uses the current directory, you define their commands by editing the config directly, e.g. via `taco config`.

An alias can also point to another path-based project, in case you want one project to inherit the commands of another:

```sh
cd ~/projects/my-app
taco alias /Users/robin/projects/other-app
# Added "/Users/robin/projects/other-app" capabilities in /Users/robin/projects/my-app
```

#### Unalias – `taco unalias {name}`

Removes an alias from the current project again:

```sh
cd ~/projects/project-a
taco unalias vitest
# Removed "vitest" capabilities from /Users/robin/projects/project-a
```

If the alias is attached to a parent directory instead, taco tells you where it is attached and how to remove it there.

#### Print – `taco print`

A flat list of every command available in the current directory, after resolution — what you see is what runs:

```sh
taco print
# Available commands:
#
# taco build  cargo build --release
# taco tdd    ./node_modules/.bin/vitest --watch
# taco test   ./node_modules/.bin/vitest
#
# 3 commands
```

Add `--verbose` (or `-v`) to see where every command comes from: a tree that mirrors the resolution order (see [Which command wins](#which-command-wins)), the current project first, with every parent directory, alias and `.taco.json` nested one level deeper. The first definition of a command is the one that runs; definitions further down that lost are greyed out and tagged as `(shadowed)`.

```sh
taco print --verbose
# Available commands:
#
# /Users/robin/github.com/RobinMalfait/taco
# ├─ taco build
# │  cargo build --release
# │
# └─ /Users/robin/github.com
#    ├─ taco tdd
#    │  ./node_modules/.bin/vitest --watch
#    ├─ taco build (shadowed)
#    │  ./node_modules/.bin/esbuild
#    └─ taco test
#       ./node_modules/.bin/vitest
#
# 4 commands
```

Or..

```sh
taco print --json
# {
#   "ls": "ls -lah",
#   "test": "./node_modules/.bin/vitest"
# }
```

#### Remove – `taco rm {name}`

```sh
taco rm ls
# Removed alias "ls"
```

`taco rm` only removes commands defined in the current directory itself. If the command is inherited from a parent directory or an alias, taco tells you where it is defined and how to remove it there.

With `--local`, this removes the command from the `.taco.json` of the current directory instead; removing the last command also removes the file.

#### Config – `taco config`

Opens `~/.config/taco/taco.json` in your default editor (`$VISUAL`, falling back to `$EDITOR`). This is the easiest way to define the commands of named projects like `vitest`, since `taco add` always works on the current directory.

The config is validated when you close the editor, so mistakes are caught immediately instead of at the next `taco` invocation. When the edit left the config broken, taco asks what to do next: re-open the editor to fix it (the default), restore the config from before the edit, or keep the broken file.

With `--local`, this opens the `.taco.json` of the current directory instead (creating it when missing), with the same validation and recovery prompt.

Set the `TACO_CONFIG` environment variable to use a different config file location.

#### Doctor – `taco doctor`

Configs accumulate rot over time. `taco doctor` checks for project directories that no longer exist, aliases attached to directories that are gone, and aliases pointing to projects that are not defined. Presets that are never aliased are reported too, but only informationally.

```sh
taco doctor
# Checking /Users/robin/.config/taco/taco.json
#
# Project directories that no longer exist:
#   ∙ /Users/robin/github.com/old-project (2 commands)
#
# 1 issue found. Run `taco doctor --fix` to clean up.
```

Run `taco doctor --fix` to remove the offending entries — you are asked for confirmation per category. When issues remain, `taco doctor` exits with a non-zero exit code.

#### Completions – `taco completions {shell}`

Prints a completion script for your shell (`zsh`, `bash` or `fish`). The completions are directory-aware:

- `taco <TAB>` completes the commands available in the current directory (including inherited ones), next to the built-in subcommands.
- `taco edit <TAB>` and `taco which <TAB>` complete the same list.
- `taco rm <TAB>` only completes commands defined in the current directory itself.
- `taco alias <TAB>` completes the projects from your config.
- `taco unalias <TAB>` completes the aliases attached to the current directory or one of its parents.

In zsh and fish, each command also shows what it runs as its description.

##### zsh

Add this to your `~/.zshrc`, after `compinit` is loaded:

```sh
source <(taco completions zsh)
```

Or write it to a directory on your `$fpath`:

```sh
taco completions zsh > ~/.zfunc/_taco
```

##### bash

Add this to your `~/.bashrc`:

```sh
eval "$(taco completions bash)"
```

##### fish

```sh
taco completions fish > ~/.config/fish/completions/taco.fish
```

#### Global flags

Every command accepts a `--pwd {path}` flag to act as if taco was run from that directory, instead of the current working directory.

```sh
taco print --pwd ~/projects/php_project_a
```

---

Inspired by the awesome [Projector](https://github.com/ThePrimeagen/projector) tool by [ThePrimeagen](https://github.com/ThePrimeagen)!
