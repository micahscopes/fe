interface Window {
    readonly attribute unsigned long innerWidth;
    readonly attribute Document document;
};

interface Document {
    readonly attribute Element documentElement;
};

interface Element {
    readonly attribute unsigned long childElementCount;
    attribute boolean hidden;
};
