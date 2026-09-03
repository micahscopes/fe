[Exposed=(Window, Worker), SecureContext]
interface GPUQueue {
    Promise<undefined> onSubmittedWorkDone();
};
