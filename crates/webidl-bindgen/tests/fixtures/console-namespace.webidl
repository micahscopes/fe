[Exposed=(Window,Worker)]
namespace console {
    readonly attribute unsigned long level;
    undefined log(DOMString message);
};

partial namespace console {
    [SecureContext] undefined log(boolean value);
    unsigned long timeStamp(DOMString label);
};
