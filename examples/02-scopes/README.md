<!-- Written by Richard Christopher, Copyright 2026 NeoTec, LLC -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Scopes, visibility, and crossing a file

```sh
yamlfy check examples/02-scopes
```

A project is a **directory tree**, and the tree *is* the scope hierarchy. There is no
separate module system to keep in sync with it.

```
02-scopes/
  api/api.yfy        namespace shop::api    public
  vault/secret.yfy   namespace shop::vault  private
  app/app.yfy        namespace shop::app    public
```

## Both axes are closed by default

`private` and `immutable` unless a scope says otherwise, and the same one level down:
**a bare member is private and immutable**, so `pub` and `mut` are opt-in.

```yfy
--- !type &Service
pub port: !!int 80      # visible outside this scope
internal_pool: 4        # not
```

That is deliberate. Making access explicit is what lets a reader of `api.yfy` know what
the rest of the project can depend on by reading one file.

## The import crosses the boundary, not the operator

`app.yfy` writes `imports: [api/api.yfy]`, and after that an ordinary alias reaches what
that file defines:

```yfy
--- !node &Storefront
extends: *Service
pub port: 8443
```

This is the whole cross-file inheritance mechanism, and it leaves the anchor rules
untouched: by the time `*Service` is written, `Service` **is** a definition of this
document. Aliases still never cross a document boundary.

## An import is not a visibility grant

`vault/` is `private`. Adding it to `app.yfy`'s imports does not open it:

```
error[E0241] app/app.yfy:11:24: `vault/secret.yfy` names a file this scope cannot
  see; an import is not a visibility grant, so nothing is installed
  note: vault/secret.yfy:6:13 `02-scopes/vault` is declared `private` and
    `02-scopes/app` is outside it; visibility composes over the whole path
    from the root
```

Two things worth noticing in that message. It names the **outermost** closed scope on
the path, because opening anything below it would change nothing — so it sends you to
the line that actually decides. And visibility **composes over the whole path**: a
`public` node inside a `private` directory is reachable from within that directory and
nowhere else, which is the ordinary "public member of a private class" case, settled
once and applied at every level.

Try it: add `vault/secret.yfy` to the `imports:` list in `app/app.yfy`.
