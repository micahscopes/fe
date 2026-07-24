# Bounded actor request admission

The browser actor endpoint has two explicit saturation policies. The default
remains immediate rejection:

```js
createActorEndpoint({ transport, requestSchema, resultSchema, maxPending: 32 })
```

Once `maxPending` requests are active, another request fails with
`FE_ACTOR_BUSY`. Applications that need producer-facing backpressure can opt
into a bounded FIFO:

```js
createActorEndpoint({
  transport,
  requestSchema,
  resultSchema,
  maxPending: 2,
  maxQueued: 8,
  saturation: "wait",
})
```

At most `maxPending` requests have crossed the transport boundary and at most
`maxQueued` requests wait locally. A queued request has not called
`transport.send`, so transferable payloads remain owned by the caller until
the request reaches an active slot. Admission is FIFO. Completion, active
abort, and synchronous send failure release a slot and immediately drain the
next queued request. Queue overflow deterministically fails with
`FE_ACTOR_BUSY`.

Request IDs are reserved when work enters the local queue, preventing duplicate
IDs from occupying multiple positions. Aborting queued work removes it without
calling `transport.send` or `transport.cancel`. Close and reset reject both
active and queued work; queued work is never transferred or cancelled.
`pendingCount()` reports active requests and `queuedCount()` reports unsent
requests.

This is bounded admission for ordinary request/result actor messages. It is not
a stream protocol, an async iterator, a resource handle, or a claim of
end-to-end flow control inside a handler. The GPU broker's separate
one-active/latest-pending supersession policy is unchanged.
