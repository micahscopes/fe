[Exposed=*]
interface Event {
  constructor(DOMString type, optional EventInit eventInitDict = {});
  readonly attribute DOMString type;
  sequence<EventTarget> composedPath();
  readonly attribute boolean bubbles;
  [LegacyUnforgeable] readonly attribute boolean isTrusted;
};

dictionary EventInit {
  boolean bubbles = false;
  boolean cancelable = false;
  boolean composed = false;
};

[Exposed=*]
interface EventTarget {
  constructor();
  undefined addEventListener(DOMString type, EventListener? callback, optional (AddEventListenerOptions or boolean) options = {});
  boolean dispatchEvent(Event event);
};

callback interface EventListener {
  undefined handleEvent(Event event);
};

dictionary EventListenerOptions {
  boolean capture = false;
};

dictionary AddEventListenerOptions : EventListenerOptions {
  boolean passive;
  boolean once = false;
};

interface Node : EventTarget {
  const unsigned short ELEMENT_NODE = 1;
  const unsigned short DOCUMENT_NODE = 9;
  readonly attribute unsigned short nodeType;
  readonly attribute DOMString nodeName;
  readonly attribute boolean isConnected;
  readonly attribute Document? ownerDocument;
  boolean hasChildNodes();
};

interface Document : Node {
  constructor();
  [SameObject] readonly attribute DOMImplementation implementation;
  readonly attribute USVString URL;
  readonly attribute DOMString contentType;
  readonly attribute Element? documentElement;
  [CEReactions, NewObject] Element createElement(DOMString localName, optional (DOMString or ElementCreationOptions) options = {});
  [NewObject] Event createEvent(DOMString interface);
};

dictionary ElementCreationOptions {
  DOMString is;
};

[Exposed=Window]
interface DOMImplementation {
  boolean hasFeature();
};

interface Element : Node {
  readonly attribute DOMString localName;
  readonly attribute DOMString tagName;
  [CEReactions] attribute DOMString id;
  DOMString? getAttribute(DOMString qualifiedName);
  boolean hasAttribute(DOMString qualifiedName);
  Element? closest(DOMString selectors);
  boolean matches(DOMString selectors);
};

interface Window : EventTarget {
  [LegacyUnforgeable] readonly attribute Document document;
  attribute DOMString name;
  undefined close();
  readonly attribute boolean closed;
  undefined focus();
  undefined blur();
};
