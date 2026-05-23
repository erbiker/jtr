---
name: Recipe proposal
about: Propose a new recipe for the curated registry
title: 'recipe: '
labels: 'recipe'
assignees: ''
---

## Recipe name

<!-- Lowercase, hyphenated. Use a language prefix for language-specific recipes (rust-*, node-*, etc.). -->

## What it does

<!-- One sentence. What problem does this recipe solve for users? -->

## Why it belongs in the curated registry

<!-- Recipes should solve real, frequently-recurring problems. "I use this in every project I write" is a strong signal. "I made this for my one project" is not. -->

## Proposed snippet

```just
# Paste the proposed `just` recipe here, exactly as it would appear in the manifest snippet.
my-recipe-name:
    @echo "..."
```

## Dependencies

<!-- What binaries does this shell out to? Anything users would need to install first? -->
- shells out to: `docker`, `psql`, ...

## Targets

- [ ] `just`
- [ ] `task` (note: Taskfile target writes aren't supported yet — see the roadmap)

## Have you validated it?

- [ ] Tested locally by running `just validate-index` after adding the manifest
- [ ] Recipe names don't collide with any existing recipe in `jtr-index/`
- [ ] No `curl | bash` patterns or other arbitrary-code-from-the-internet fetches at runtime
