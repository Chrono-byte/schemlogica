// Sample boolean-only JavaScript input for schemlogica
// Allowed features: `let` declarations, boolean literals, identifiers,
// unary `!`, logical `&&` / `||`, conditional (ternary) `?:`, and `==` / `!=`.

// Initial signals
let a = true;
let b = false;

// Basic logical operations
let c = a && !b;        // true && !false -> true
let d = a || b;         // true || false -> true

// Conditional (ternary)
let e = (a && b) ? false : true; // a&&b is false -> e is true

// Equality checks
let same = a == d;      // true == true -> true
let different = a != b; // true != false -> true

// Composed expression using previously defined vars
let intermediate = (c || d) && !different;

// Re-assignment expression as an expression statement (allowed)
intermediate = intermediate ? e : b;

// Final expression statement (can serve as program output)
intermediate;
