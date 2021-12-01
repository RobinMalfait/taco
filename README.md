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

Scripts inherit scripts from **parent** directories. This allows you to set the `npm run test` only once in a shared folder. In my case, I did this in a `~/github.com/tailwindlabs` folder.

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

### API

#### Add – `taco add {name} -- {command}`

```sh
taco add ls -- ls -lah
# Aliased "ls" to "ls -lah" in /Users/robin
```

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

Or if you want to look at the command that is going to be executed use the `--print` flag.
```sh
taco ls --print
# ls -lah
```

#### Print – `taco print`

```sh
taco print
# Available commands:
#
#   taco test
#     ./node_modules/.bin/jest
#
#   taco ls
#     ls -lah
#
# 2 commands
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

---

Inspired by the awesome [Projector](https://github.com/ThePrimeagen/projector) tool by [ThePrimeagen](https://github.com/ThePrimeagen)!
