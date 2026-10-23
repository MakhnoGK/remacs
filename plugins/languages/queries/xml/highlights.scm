; XML, as HTML: elements as tags, declarations and instructions as the preprocessor.

(STag (Name) @tag)
(ETag (Name) @tag)
(EmptyElemTag (Name) @tag)
(Attribute (Name) @attribute)
(AttValue) @string
[(EntityRef) (CharRef)] @constant
(CDSect) @string

[(XMLDecl) (PI) (StyleSheetPI) (XmlModelPI) (doctypedecl)] @preproc
(doctypedecl (Name) @type)
[(PubidLiteral) (SystemLiteral)] @string

(Comment) @comment
