Recognize `Constraint` as a distinct generic parameter kind, including higher-kinded forms such as `* -> Constraint`. An ordinary value type cannot fill a constraint parameter. This introduces goal-kind syntax, not public proof construction or evidence consumption.

Concrete trait goals can appear in constraint-kinded positions, with the subject as the first argument. Goal terms preserve nominal subject and associated-binding identity through substitution. Merely constructing a goal description supplies no trait evidence.
