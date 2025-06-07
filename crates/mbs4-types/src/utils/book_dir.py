import os
import unicodedata

def initials(name):
    names = name.split()
    return ' '.join(map(lambda n: n[0].upper(), names))

class Ebook():
    @property
    def authors_str(self):
        if not self.authors:
            return 'No Authors'
        if len(self.authors) == 1:
            return '{a.last_name} {a.first_name}'.format(a=self.authors[0])\
                 if self.authors[0].first_name else self.authors[0].last_name
        else:
            l = len(self.authors)
            authors = []
            for i in range(min(3, l)):
                authors.append('{a.last_name} {initials}'.format(a=self.authors[i],
                                initials=initials(self.authors[i].first_name))\
                               if self.authors[i].first_name else self.authors[i].last_name
                               )
            s = ', '.join(authors)
            if l > 3:
                s += ' and others'
            return s

    def __repr__(self):
        return super(Ebook, self).__repr__(['title'])

def ebook_base_dir(ebook):
    return os.path.split(norm_file_name(ebook))[0]


nd_charmap = {
    u'\N{Latin capital letter AE}': 'AE',
    u'\N{Latin small letter ae}': 'ae',
    u'\N{Latin capital letter Eth}': 'D',
    u'\N{Latin small letter eth}': 'd',
    u'\N{Latin capital letter O with stroke}': 'O',
    u'\N{Latin small letter o with stroke}': 'o',  #
    u'\N{Latin capital letter Thorn}': 'Th',
    u'\N{Latin small letter thorn}': 'th',
    u'\N{Latin small letter sharp s}': 's',
    u'\N{Latin capital letter D with stroke}': 'D',
    u'\N{Latin small letter d with stroke}': 'd',
    u'\N{Latin capital letter H with stroke}': 'H',
    u'\N{Latin small letter h with stroke}': 'h',
    u'\N{Latin small letter dotless i}': 'i',
    u'\N{Latin small letter kra}': 'k',
    u'\N{Latin capital letter L with stroke}': 'L',
    u'\N{Latin small letter l with stroke}': 'l',
    u'\N{Latin capital letter Eng}': 'N',
    u'\N{Latin small letter eng}': 'n',
    u'\N{Latin capital ligature OE}': 'Oe',
    u'\N{Latin small ligature oe}': 'oe',
    u'\N{Latin capital letter T with stroke}': 'T',
    u'\N{Latin small letter t with stroke}': 't',
}


def remove_diacritics(text):
    "Removes diacritics from the string"
    if not text:
        return text
    s = unicodedata.normalize('NFKD', text)
    b = []
    for ch in s:
        if unicodedata.category(ch) != 'Mn':
            if ch in nd_charmap:
                b.append(nd_charmap[ch])
            elif ord(ch) < 128:
                b.append(ch)
            else:
                b.append(' ')
    return ''.join(b)

def norm_file_name(ebook, ext=''):
    
    new_name_rel = norm_file_name_base(ebook)
    for ch in [':', '*', '%', '|', '"', '<', '>', '?', '\\']:
        new_name_rel = new_name_rel.replace(ch, '')
    new_name_rel += '.' + ext

    return new_name_rel

def _safe_file_name(name):
    return name.replace('/', '-')

BOOKS_FILE_SCHEMA = "%(author)s/%(title)s(%(language)s)/%(author)s - %(title)s"
BOOKS_FILE_SCHEMA_SERIE = "%(author)s/%(serie)s/%(serie)s %(serie_index)d - %(title)s(%(language)s)/%(author)s - %(serie)s %(serie_index)d - %(title)s"

def norm_file_name_base(ebook):
    data = {'author': _safe_file_name(ebook.authors_str),
            'title': _safe_file_name(ebook.title),
            'language': ebook.language.code,
            }
    if ebook.series:
        data.update({'serie': _safe_file_name(ebook.series.title),
                    'serie_index': ebook.series_index or 0})
    if ebook.series and BOOKS_FILE_SCHEMA_SERIE:
        new_name_rel = BOOKS_FILE_SCHEMA_SERIE % data
        # TODO: might need to spplit base part
    else:
        new_name_rel = BOOKS_FILE_SCHEMA % data
    new_name_rel = remove_diacritics(new_name_rel)
    assert(len(new_name_rel) < 4096)
    return new_name_rel