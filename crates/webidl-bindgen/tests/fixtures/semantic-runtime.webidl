callback EventListener = undefined (Event event);

interface Event {
  readonly attribute DOMString type;
};

interface EventTarget {
  undefined addEventListener(DOMString type, EventListener callback);
  Event echo(Event event);
};
