# Create Git Tag v1.1.1

## Context
One bugfix commit since v1.1.0: `e149b682 fix(proxy): remove non-interactive TTY guard blocking device flows in Docker`

## Steps
1. Create annotated tag: `git tag -a v1.1.1 -m "v1.1.1: fix device flow TTY guard in Docker"`
2. Push tag: `git push origin v1.1.1`
