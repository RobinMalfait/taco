## Taco

It's a wrapper around your commands!

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
