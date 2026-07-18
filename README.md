## Taco

> It's a wrapper around your commands!

### Eh? What are you talking about...

Let's imagine you have 2 projects, and you want to run `tests` in each project.

1. `Project A`, is a Laravel PHP project, so you want to use `phpunit` or `pest`.
2. `Project b`, is a JavaScript project, so you want to use `jest` or `npm run test`.

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
      "tdd": "./node_modules/.bin/jest --watch",
      "test": "./node_modules/.bin/jest"
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

#### Edit – `taco edit {name}`

```sh
taco edit ls
```

This will open your default editor (`$VISUAL`, falling back to `$EDITOR`) to edit the command. The current command will be pre-filled. Lines with `#` at the start will be ignored.

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
# Runs: ./node_modules/.bin/jest --watch
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
#     ./node_modules/.bin/jest
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

Commands are grouped by where they are defined — parent directories and aliases first, the current project last. Commands that are overridden by a deeper project only show up in the group that won.

```sh
taco print
# Available commands:
#
# /Users/robin/github.com
#   ├─ taco tdd
#   │    ./node_modules/.bin/jest --watch
#   └─ taco test
#        ./node_modules/.bin/jest
#
# /Users/robin/github.com/RobinMalfait/taco
#   └─ taco build
#        cargo build --release
#
# 3 commands
```

Or..

```sh
taco print --json
# {
#   "ls": "ls -lah",
#   "test": "./node_modules/.bin/jest"
# }
```

#### Remove – `taco rm {name}`

```sh
taco rm ls
# Removed alias "ls"
```

`taco rm` only removes commands defined in the current directory itself. If the command is inherited from a parent directory or an alias, taco tells you where it is defined and how to remove it there.

#### Config – `taco config`

Opens `~/.config/taco/taco.json` in your default editor (`$VISUAL`, falling back to `$EDITOR`). This is the easiest way to define the commands of named projects like `vitest`, since `taco add` always works on the current directory.

The config is validated when you close the editor, so mistakes are caught immediately instead of at the next `taco` invocation.

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
