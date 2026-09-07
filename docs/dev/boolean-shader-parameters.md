# Boolean shader parameters

The composed Patch demo introduced a typed scene policy containing booleans.
Fe/Wasm accepted it, but the GPU stage rejected flattened argument 62 (`i1`):
Sonatina previously prohibited boolean entry parameters in storage records.

Sonatina `6bda54bb470c25e89658b614e17898c2af687957` gives these entries a
host-shareable four-byte `u32` carrier and decodes nonzero to logical true in
the entry body. The reflected binding member reports `U32`, width four; shader
helpers still receive logical booleans. This is an ABI implementation, not a
change to authored Fe types or a browser-specific policy conversion.

The focused release regression group has five passing tests: scalar/grid/
fullscreen layouts, real GPU false/true/noncanonical-nonzero decoding, explicit
compute parameter-controlled writes (shader validation), an unused boolean
parameter's stable layout, and existing logical/loop behavior. The old negative
test has been converted to positive layout coverage, not deleted.

GPU execution used AMD Radeon 780M/RADV with the system Vulkan loader and ICD.
The full compiler suite and final composed-Patch browser build are separate
integration gates; passing these focused tests does not establish them.
