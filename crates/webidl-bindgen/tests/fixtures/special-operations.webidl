[Exposed=Window]
interface DOMStringMap {
    getter DOMString? (DOMString name);
    setter undefined (DOMString name, DOMString value);
    deleter undefined (DOMString name);
};

[Exposed=Window]
interface Storage {
    getter DOMString? getItem(DOMString key);
    setter undefined setItem(DOMString key, DOMString value);
    deleter undefined removeItem(DOMString key);
};

[Exposed=Window]
interface URL {
    stringifier;
};
