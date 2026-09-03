typedef [EnforceRange] unsigned long long GPUSize64;

[Exposed=(Window, Worker), SecureContext]
interface GPUBuffer {};

[Exposed=(Window, Worker), SecureContext]
interface GPUQueue {
    undefined writeBuffer(
        GPUBuffer buffer,
        GPUSize64 bufferOffset,
        AllowSharedBufferSource data,
        optional GPUSize64 dataOffset = 0,
        optional GPUSize64 size);
};
