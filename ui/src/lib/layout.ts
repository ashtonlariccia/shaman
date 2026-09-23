/**
 * Fixed dimensions shared between the chrome components.
 *
 * `RAIL_WIDTH` in particular is needed in three places that must agree: the
 * sidebar sets it as its collapsed width, App uses it to size the sidebar, and
 * the title bar reserves exactly that much for its logo so the menus begin
 * clear of the rail. Defining it once is what keeps them aligned.
 */

/**
 * Width of the collapsed sidebar rail.
 *
 * Wide enough for the kind glyph and the collapse toggle to sit centred with
 * room to breathe, narrow enough to read as a rail rather than a thin sidebar.
 */
export const RAIL_WIDTH = 42;
