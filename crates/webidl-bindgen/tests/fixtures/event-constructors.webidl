[Exposed=Window, NamedConstructor=LegacyEvent(DOMString type)]
interface Event {
    constructor(DOMString type);
    [SecureContext] constructor(DOMString type, boolean bubbles);
    readonly attribute DOMString type;
};
