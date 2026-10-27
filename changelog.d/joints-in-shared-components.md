### Assemblies

- A joint between two parts inside one copy of a component placed more than once is converted
  once, as that component's own joint, instead of being left out. A joint from such a part to
  something outside its copy holds the whole copy, with a note.
- A joint to a part inside a design inserted into a component, which Fusion names by the
  part's key alone, joins that part instead of the component that holds it.
- A component that is empty in the source is written as an empty component, so the joints to
  it are kept.
