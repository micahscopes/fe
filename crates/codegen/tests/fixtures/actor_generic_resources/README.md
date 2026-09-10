# Generic resource boundary probe

`read<T, N>` and `total<N>` test ordinary user generics across the Fe/Sonatina
boundary. Two equal-shaped resources have different identity. Repeated calls
using one resource should share a loop helper; calls using the other resource
need a distinct physical specialization.

The companion `resource_boundary` test checks emitted sharing and executes the
shader with `left = 2` and `right = 7`. Each invocation must produce
`(3 + 5) * 2 + (3 + 5) * 7 = 72`, without shader traps.

An independently compiled source variant chooses the input resource at runtime.
It must either reject that unsupported identity join explicitly or execute the
correct lane-dependent result. Successful compilation by itself is insufficient.
Use an independent source module for that variant: mutating the initialized
ingot's backing file in the test database did not reliably select the replacement
as the compiled root during this investigation.

This fixture does not claim arbitrary resource joins or a generic GPU `map`.
Generic type substitution and physical resource selection are different tasks.
