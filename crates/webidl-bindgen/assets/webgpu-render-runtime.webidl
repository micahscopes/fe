dictionary GPUObjectDescriptorBase {
    USVString label = "";
};

dictionary GPUBufferDescriptor : GPUObjectDescriptorBase {
    required GPUSize64 size;
    required GPUBufferUsageFlags usage;
    boolean mappedAtCreation = false;
};

typedef [EnforceRange] unsigned long long GPUSize64;
typedef [EnforceRange] unsigned long GPUSize32;
typedef [EnforceRange] unsigned long GPUBufferUsageFlags;

[Exposed=(Window, Worker), SecureContext]
interface GPUBuffer {};

[Exposed=(Window, Worker), SecureContext]
interface GPUDevice {
    GPUBuffer createBuffer(GPUBufferDescriptor descriptor);
};

[Exposed=(Window, Worker), SecureContext]
interface GPUQueue {
    Promise<undefined> onSubmittedWorkDone();
    undefined writeBuffer(
        GPUBuffer buffer,
        GPUSize64 bufferOffset,
        AllowSharedBufferSource data,
        optional GPUSize64 dataOffset = 0,
        optional GPUSize64 size);
};

[Exposed=(Window, Worker), SecureContext]
interface GPURenderPassEncoder {};

interface mixin GPURenderCommandsMixin {
    undefined draw(
        GPUSize32 vertexCount,
        optional GPUSize32 instanceCount = 1,
        optional GPUSize32 firstVertex = 0,
        optional GPUSize32 firstInstance = 0);
    undefined drawIndirect(GPUBuffer indirectBuffer, GPUSize64 indirectOffset);
};

GPURenderPassEncoder includes GPURenderCommandsMixin;
