/*
 * BERT WordPiece tokenizer driven by a HuggingFace tokenizer.json, plus
 * id2label from config.json. See tokenizer.c for the supported subset.
 */
#ifndef BERT_TOKENIZER_H
#define BERT_TOKENIZER_H

#include <stddef.h>
#include <stdint.h>

typedef struct bert_tokenizer bert_tokenizer;

typedef struct bert_labels {
  char **names; /* names[id], NULL where config has no entry */
  size_t n;
} bert_labels;

int bert_tokenizer_load(const char *tokenizer_json_path, bert_tokenizer **out,
                        char *err, size_t errlen);
void bert_tokenizer_free(bert_tokenizer *t);

int32_t bert_tokenizer_pad_id(const bert_tokenizer *t);

/*
 * Encodes n texts as one padded batch (padding to the longest sequence with
 * the pad id; attention mask 1 for real tokens, 0 for padding). On success
 * *ids and *mask are malloc'd arrays of n * *seq_len int64 values (row-major).
 */
int bert_tokenizer_encode_batch(const bert_tokenizer *t, const char *const *texts, size_t n,
                                int64_t **ids, int64_t **mask, size_t *seq_len,
                                char *err, size_t errlen);

int bert_load_id2label(const char *config_json_path, bert_labels *out, char *err, size_t errlen);
void bert_labels_free(bert_labels *l);

#endif /* BERT_TOKENIZER_H */
