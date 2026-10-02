Retain lexical premises when trait obligations are checked later, so qualified trait calls inside a compile-time evidence scope succeed without leaking that scope's requirements to surrounding code.
