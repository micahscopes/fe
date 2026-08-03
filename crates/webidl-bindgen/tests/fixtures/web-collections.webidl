[Exposed=(Window,Worker)]
interface URLSearchParams {
    iterable<DOMString, DOMString>;
};

[Exposed=(Window,Worker)]
interface Headers {
    iterable<DOMString, DOMString>;
};

[Exposed=Window]
interface DOMTokenList {
    iterable<DOMString>;
};

interface ReadonlyRegistry {
    readonly maplike<DOMString, DOMString>;
};

interface MutableFeatureSet {
    setlike<DOMString>;
};
