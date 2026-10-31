### Assemblies

- A converted assembly writes each sketch under the part that owns it instead of in the
  assembly root, once per part however many times it is placed, including sketches of a
  component that holds no bodies. Sketches with no owner, such as those of the design's own
  root component, stay in the root, and the report counts them.
  Sketches a part's history builds nothing on, and sketches the source hides, start hidden.
