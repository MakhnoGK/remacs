; Dockerfile and Containerfile.

[
  "ADD" "ARG" "AS" "CMD" "COPY" "CROSS_BUILD" "ENTRYPOINT" "ENV" "EXPOSE" "FROM"
  "HEALTHCHECK" "LABEL" "MAINTAINER" "NONE" "ONBUILD" "RUN" "SHELL" "STOPSIGNAL" "USER"
  "VOLUME" "WORKDIR"
] @keyword

(image_name) @type
[(image_tag) (image_digest)] @constant
(image_alias) @label
(arg_instruction name: (unquoted_string) @constant)
(env_pair name: (unquoted_string) @constant)
(label_pair key: (_) @property)
(expose_port) @number
[(param) (mount_param)] @attribute

[(double_quoted_string) (single_quoted_string) (json_string)] @string
(escape_sequence) @string.escape
(expansion) @constant
[(heredoc_marker) (heredoc_end)] @label

(comment) @comment
