dictionary GPUObjectDescriptorBase {
    USVString label = "";
};

dictionary GPUBufferDescriptor : GPUObjectDescriptorBase {
    required GPUSize64 size;
    required GPUBufferUsageFlags usage;
    boolean mappedAtCreation = false;
};

typedef [EnforceRange] unsigned long long GPUSize64;
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
