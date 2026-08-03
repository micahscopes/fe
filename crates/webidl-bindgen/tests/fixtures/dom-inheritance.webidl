[Exposed=Window]
interface EventTarget {
    boolean dispatchEvent(boolean trusted);
};

[Exposed=Window]
interface Node : EventTarget {
    readonly attribute unsigned long nodeType;
    Node appendChild(Node child);
};

[Exposed=Window]
interface Element : Node {
    attribute boolean hidden;
    undefined focus();
};
