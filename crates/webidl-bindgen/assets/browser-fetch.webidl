[Global=Window, Exposed=Window]
interface Window {
    Promise<Response> fetch(USVString input);
};

[Exposed=Window]
interface Response {
    readonly attribute unsigned short status;
    readonly attribute boolean ok;
    readonly attribute USVString url;
    Promise<USVString> text();
    Promise<ArrayBuffer> arrayBuffer();
};
