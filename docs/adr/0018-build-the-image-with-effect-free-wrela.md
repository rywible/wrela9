# Build the Image with effect-free Wrela

Every Project has one reachable `@image` Image Constructor written as ordinary Wrela and evaluated by the compiler. It produces the closed Image graph by selecting high-level Image Facilities, declaring bounds, creating Actors, and choosing the root behavior. Compile-time execution cannot inspect ambient host files, environment variables, clocks, entropy, or the network; every build input must be explicit source or immutable Project content.

Wrela9 initially has no manifest, package manager, dependency resolver, or external source dependencies. Modules and assets follow conventional locations in the Project source tree. This keeps Image semantics in the language and postpones package machinery until a concrete dependency model is needed.
