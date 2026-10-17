/**
 * Stable application state surface. The reactive singleton remains the source of
 * truth during the first migration pass; feature slices can be added behind this
 * facade without forcing consumers to change imports twice.
 */
export * from '../../store';
