"""Small local English syntax model; statistical labels remain learning aids."""
import spacy

IRREGULAR = {
    'be': ['was / were', 'been'],
    'have': ['had', 'had'],
    'do': ['did', 'done'],
    'go': ['went', 'gone'],
    'come': ['came', 'come'],
    'get': ['got', 'got / gotten'],
    'give': ['gave', 'given'],
    'take': ['took', 'taken'],
    'make': ['made', 'made'],
    'see': ['saw', 'seen'],
    'know': ['knew', 'known'],
    'think': ['thought', 'thought'],
    'say': ['said', 'said'],
    'tell': ['told', 'told'],
    'find': ['found', 'found'],
    'leave': ['left', 'left'],
    'feel': ['felt', 'felt'],
    'keep': ['kept', 'kept'],
    'meet': ['met', 'met'],
    'read': ['read', 'read'],
    'write': ['wrote', 'written'],
    'speak': ['spoke', 'spoken'],
    'eat': ['ate', 'eaten'],
    'drink': ['drank', 'drunk'],
    'run': ['ran', 'run'],
    'begin': ['began', 'begun'],
    'become': ['became', 'become'],
    'break': ['broke', 'broken'],
    'bring': ['brought', 'brought'],
    'buy': ['bought', 'bought'],
    'build': ['built', 'built'],
    'catch': ['caught', 'caught'],
    'choose': ['chose', 'chosen'],
    'draw': ['drew', 'drawn'],
    'drive': ['drove', 'driven'],
    'fall': ['fell', 'fallen'],
    'forget': ['forgot', 'forgotten'],
    'hear': ['heard', 'heard'],
    'hold': ['held', 'held'],
    'lose': ['lost', 'lost'],
    'pay': ['paid', 'paid'],
    'put': ['put', 'put'],
    'send': ['sent', 'sent'],
    'sit': ['sat', 'sat'],
    'sleep': ['slept', 'slept'],
    'stand': ['stood', 'stood'],
    'teach': ['taught', 'taught'],
    'understand': ['understood', 'understood'],
    'wear': ['wore', 'worn'],
    'win': ['won', 'won'],
}
POS={'NOUN':'Существительное','PROPN':'Имя','PRON':'Местоимение','ADJ':'Прилагательное','ADV':'Наречие','VERB':'Глагол','AUX':'Вспом. глагол','ADP':'Предлог','PART':'Частица','DET':'Определитель','CCONJ':'Союз','SCONJ':'Союз','NUM':'Числительное','INTJ':'Междометие'}
ROLES={'nsubj':'Подлежащее','nsubjpass':'Подлежащее','csubj':'Подлежащее','dobj':'Объект','obj':'Объект','dative':'Косвенный объект','pobj':'Объект предлога','amod':'Определение','advmod':'Обстоятельство','prt':'Частица фразового глагола'}

class Grammar:
    """Load the small parser once; exclude named entity recognition to reduce work."""
    def __init__(self):
        self.nlp=spacy.load('en_core_web_sm',exclude=['ner'])
    def analyze(self,text):
        """Return token offsets, POS, dependency roles and actual grammatical verb forms."""
        tokens=[]
        for word in self.nlp(text[:12000]):
            if word.is_space or word.is_punct:continue
            verb=word.pos_ in ('VERB','AUX')
            form={'VB':'V1','VBP':'V1','VBZ':'V1+s','VBD':'V2','VBN':'V3','VBG':'V-ing'}.get(word.tag_,'') if verb else ''
            forms=[word.lemma_]+IRREGULAR[word.lemma_] if verb and word.lemma_ in IRREGULAR else []
            interjection=word.text.lower() in ('yup','yep','yeah','uh','hmm','oops')
            role='' if interjection else ROLES.get(word.dep_,'Сказуемое' if word.dep_=='ROOT' and verb else '')
            pos='INTJ' if interjection else 'PART' if word.dep_=='prt' else word.pos_
            tokens.append({'text':word.text,'start':word.idx,'end':word.idx+len(word.text),'lemma':word.lemma_,'pos':pos,'label':POS.get(pos,pos),'role':role,'verb_form':form,'irregular':bool(forms),'forms':forms,'dependency':word.dep_})
        return {'tokens':tokens,'source':'spaCy en_core_web_sm 3.8 · локальная статистическая модель'}
