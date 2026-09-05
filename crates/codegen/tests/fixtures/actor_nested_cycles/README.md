# Nested GPU cycles

Six Fe stages compile once. The host interprets the Fe-derived outer job and
inner round schedule without shader or command-plan duplication.

After execution, `receipt[0]` is 17 and the following 17 words are:

```text
1 2 3 2 3 2 3 4 1 2 3 2 3 2 3 4 5
```

From the compiler root, serve this fixture with the release CLI:

```sh
target/release/fe web dev crates/codegen/tests/fixtures/actor_nested_cycles/index.html --port 8783 --no-watch
```

The `actor_construct` test proves the emitted nested plan and shared shaders.
`pass-schedule.test.mjs` checks actual host dispatch order, taper counter reset,
compact plans, and malformed/reopened/reparented cycle rejection. Browser
inspection must read the real Fe-generated receipt, not substitute a shader
that reproduces the expected values.

`browser-evidence.json` records that exact receipt from Chromium, with six
compiled stages and the portable eight-storage-binding device limit. The
companion runtime test checks this recorded evidence separately from its
synthetic host tests. Release gates passed: 47 actor tests, the compiler's
invalid-nesting unit test, and 64 runtime tests. The existing single-cycle
native execution gate also passed on llvmpipe after explicitly providing the
installed Vulkan loader and ICD to the process; default shell discovery did
not find an adapter.
