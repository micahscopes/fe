[Exposed=(Window,Worker)]
interface Event {
    const unsigned short NONE = 0;
    const unsigned short CAPTURING_PHASE = 1;
    const unsigned short AT_TARGET = 2;
    const unsigned short BUBBLING_PHASE = 3;
    readonly attribute DOMString type;
};
