## Taco

> It's a wrapper around your commands!

### Example Usage

Let's imagine you have 2 projects, and you want to run `tests` in each project.

1. `Project A`, is a Laravel PHP project, so you want to use `phpunit` or `pest`.
2. `Project b`, is a JavaScript project, so you want to use `jest` or `npm run test`.

I don't want to rember all of that...

```sh
cd ~/projects/php_project_a
taco add test -- phpunit

cd ~/projects/js_project_b
taco add test -- npm run test
```

So what happened here? We created aliases.

`~/.config/taco/taco.json`
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

Scripts also inherit scripts that are set for **parent** directories. This allows you to set the `npm run test` only once in a shared folder.

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

```
Taco 1.0

USAGE:
    Taco [FLAGS] [OPTIONS] [ARGS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -p, --print
    -V, --version    Prints version information

OPTIONS:
    -c, --config <config>    Sets a custom config file
        --pwd <pwd>          The current working directory [default: /Users/robin/github.com/RobinMalfait/taco]

ARGS:
    <command>
    <arguments>...    Arguments to passthrough

SUBCOMMANDS:
    add      Add a new command
    help     Prints this message or the help of the given subcommand(s)
    print    Print the commands
    rm       Delete an existing command
```

Inspired by the awesome [Projector](https://github.com/ThePrimeagen/projector) tool by [ThePrimeagen](https://github.com/ThePrimeagen)!
