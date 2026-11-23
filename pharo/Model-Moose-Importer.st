"Model-Moose-Importer: import a Model into Famix through model-ffi (no JSON). File in with FileStream fileIn."

FFILibrary subclass: #ModelLibrary instanceVariableNames: '' classVariableNames: '' package: 'Model-Moose-Importer'!
ModelLibrary class instanceVariableNames: 'libraryPath'!

!ModelLibrary methodsFor: 'ffi'!
abiVersion	^ self ffiCall: #(uint32 model_abi_version ())! !

!ModelLibrary methodsFor: 'reading'!
charOffsetsIn: aPath ranges: aCollectionOfQuadruples	"Answers (start, end) character offsets (1-based, inclusive) per range, or nil if the file cannot be read."	| count rangeBytes out rc |	count := aCollectionOfQuadruples size.	count = 0 ifTrue: [ ^ #(  ) ].	rangeBytes := ByteArray new: count * 16.	aCollectionOfQuadruples doWithIndex: [ :q :i |			1 to: 4 do: [ :k |				rangeBytes					unsignedLongAt: i - 1 * 16 + (k - 1 * 4) + 1					put: (q at: k) ] ].	out := ByteArray new: count * 8.	rc := self		      charOffsetsPath: aPath		      ranges: rangeBytes		      count: count		      into: out.	rc = 0 ifFalse: [ ^ nil ].	^ (1 to: count) collect: [ :i |			  {				  (out unsignedLongAt: i - 1 * 8 + 1).				  (out unsignedLongAt: i - 1 * 8 + 5) } ]! !

!ModelLibrary methodsFor: 'ffi'!
charOffsetsPath: aPath ranges: aRangesBytes count: n into: anOutBytes	^ self ffiCall: #(int model_char_offsets (String aPath, void * aRangesBytes, uint32 n, void * anOutBytes))! !

!ModelLibrary methodsFor: 'ffi'!
entityCount: aHandle	^ self ffiCall: #(uint64 model_entity_count (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
entityFiles: aHandle	^ self ffiCall: #(void * model_entity_files (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
entityFlags: aHandle	^ self ffiCall: #(void * model_entity_flags (void * aHandle))! !

!ModelLibrary methodsFor: 'reading'!
entityKeys: aHandle	^ self ffiCall: #(void * model_entity_keys (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
entityKinds: aHandle	^ self ffiCall: #(void * model_entity_kinds (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
entityNames: aHandle	^ self ffiCall: #(void * model_entity_names (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
entityParents: aHandle	^ self ffiCall: #(void * model_entity_parents (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
entityRanges: aHandle	^ self ffiCall: #(void * model_entity_ranges (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
free: aHandle	^ self ffiCall: #(void model_free (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
kindArchetypes: aHandle	^ self ffiCall: #(void * model_kind_archetypes (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
kindCount: aHandle	^ self ffiCall: #(uint64 model_kind_count (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
kindFm3: aHandle	^ self ffiCall: #(void * model_kind_fm3 (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
kindNames: aHandle	^ self ffiCall: #(void * model_kind_names (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
lastError	^ self ffiCall: #(String model_last_error ())! !

!ModelLibrary methodsFor: 'accessing platform'!
macLibraryName	^ self class libraryPath! !

!ModelLibrary methodsFor: 'ffi'!
open: aPath	^ self ffiCall: #(void * model_open (String aPath))! !

!ModelLibrary methodsFor: 'ffi'!
relationCount: aHandle	^ self ffiCall: #(uint64 model_relation_count (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
relationFrom: aHandle	^ self ffiCall: #(void * model_relation_from (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
relationKindCount: aHandle	^ self ffiCall: #(uint64 model_relation_kind_count (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
relationKindNames: aHandle	^ self ffiCall: #(void * model_relation_kind_names (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
relationKinds: aHandle	^ self ffiCall: #(void * model_relation_kinds (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
relationSiteOffsets: aHandle	^ self ffiCall: #(void * model_relation_site_offsets (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
relationTo: aHandle	^ self ffiCall: #(void * model_relation_to (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
siteCount: aHandle	^ self ffiCall: #(uint64 model_site_count (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
siteFlags: aHandle	^ self ffiCall: #(void * model_site_flags (void * aHandle))! !

!ModelLibrary methodsFor: 'ffi'!
siteRanges: aHandle	^ self ffiCall: #(void * model_site_ranges (void * aHandle))! !

!ModelLibrary methodsFor: 'reading'!
stringColumn: aPointer count: n	"Reads a column of StrRef { ptr; u32 len } (16 bytes); an absent string is nil."	^ (1 to: n) collect: [ :i |			  | offset ptr len bytes |			  offset := i - 1 * 16.			  ptr := aPointer pointerAtOffset: offset.			  len := aPointer unsignedLongAt: offset + 9.			  ptr isNull				  ifTrue: [ nil ]				  ifFalse: [						  bytes := ByteArray new: len.						  len > 0 ifTrue: [ LibC memCopy: ptr to: bytes size: len ].						  bytes utf8Decoded ] ]! !

!ModelLibrary methodsFor: 'reading'!
uint32Column: aPointer count: n	"Copies a u32 column; u32::MAX (absent) is mapped to nil."	| bytes |	n = 0 ifTrue: [ ^ #(  ) ].	bytes := ByteArray new: n * 4.	LibC memCopy: aPointer to: bytes size: n * 4.	^ (1 to: n) collect: [ :i |			  | v |			  v := bytes unsignedLongAt: i - 1 * 4 + 1.			  v = 4294967295				  ifTrue: [ nil ]				  ifFalse: [ v ] ]! !

!ModelLibrary methodsFor: 'reading'!
uint8Column: aPointer count: n	| bytes |	n = 0 ifTrue: [ ^ #(  ) ].	bytes := ByteArray new: n.	LibC memCopy: aPointer to: bytes size: n.	^ bytes asArray! !

!ModelLibrary methodsFor: 'accessing platform'!
unix64LibraryName	^ self class libraryPath! !

!ModelLibrary methodsFor: 'accessing platform'!
win32LibraryName	^ self class libraryPath! !

!ModelLibrary class methodsFor: 'accessing'!
libraryPath	^ libraryPath! !

!ModelLibrary class methodsFor: 'accessing'!
libraryPath: aStringOrFileReference	libraryPath := aStringOrFileReference asFileReference pathString! !
Object subclass: #ModelImporter instanceVariableNames: 'lib famixModel prefix sourceRoot n names files parents kinds flags ranges kindNames relFrom relTo relKinds siteOffsets siteRanges siteFlags relKindNames entities associations anchorQueue containerCache skipped acceptCache kindArchetypes fileTypes' classVariableNames: '' package: 'Model-Moose-Importer'!

!ModelImporter methodsFor: 'importing'!
accepts: anEntity as: aPropertyName of: anAssociationClass	"Does the FM3 type of that property accept the entity? A property unknown to the metamodel does not reject."	acceptCache ifNil: [ acceptCache := Dictionary new ].	^ acceptCache		  at: {				  anAssociationClass.				  aPropertyName.				  anEntity class }		  ifAbsentPut: [				  (famixModel metamodel fmTypeOf: anAssociationClass)					  allProperties					  detect: [ :p | p name = aPropertyName ]					  ifFound: [ :p |							  | t |							  t := p type implementingClass.							  (anEntity class includesBehavior: t) or: [								  anEntity class allTraits includes: t ] ]					  ifNone: [ true ] ]! !

!ModelImporter methodsFor: 'importing'!
associationSpecFor: aKind from: from to: to	"{ simple FM3 class. from property. to property (for the FM3 type check) } or nil when the kind has no association."	| implements |	aKind = 'calls' ifTrue: [ ^ #( 'Invocation' #sender #candidates ) ].	aKind = 'accesses' ifTrue: [ ^ #( 'Access' #accessor #candidates ) ].	aKind = 'declared_type' ifTrue: [		^ #( 'EntityTyping' #typedEntity #declaredType ) ].	aKind = 'references' ifTrue: [		^ #( 'Reference' #referencer #referredEntity ) ].	aKind = 'imports' ifTrue: [		^ #( 'Import' #importingEntity #importedEntity ) ].	(aKind = 'inherits' or: [ aKind = 'implements' ]) ifTrue: [			implements := (self isInterfaceLike: to) and: [					              (self isInterfaceLike: from) not and: [						              from class name ~= (prefix , 'Enum') asSymbol ] ].			^ implements				  ifTrue: [ #( 'Implementation' #implementingClass				     #interface ) ]				  ifFalse: [ #( 'Inheritance' #subclass #superclass ) ] ].	^ nil! !

!ModelImporter methodsFor: 'importing'!
classNamed: aSimpleName	^ Smalltalk at: (prefix , aSimpleName) asSymbol! !

!ModelImporter methodsFor: 'importing'!
computeFileTypes	fileTypes := Dictionary new.	1 to: n do: [ :i |			| entity file parent |			entity := entities at: i.			file := files at: i.			(entity notNil and: [					 file notNil and: [							 ((flags at: i) bitAnd: 1) = 0 and: [									 (self isNamed: 'Class' in: entity) or: [											 (self isNamed: 'Interface' in: entity) or: [													 (self isNamed: 'Enum' in: entity) or: [														 self isNamed: 'AnnotationType' in: entity ] ] ] ] ] ])				ifTrue: [						parent := (parents at: i) ifNotNil: [ :p | entities at: p + 1 ].						(parent notNil and: [							 (self isTypeEntity: parent) or: [								 self isNamed: 'Method' in: parent ] ]) ifFalse: [							fileTypes at: file ifAbsentPut: [ i ] ] ] ]! !

!ModelImporter methodsFor: 'importing'!
containerSelectorFor: child parent: parent	^ containerCache		  at: {				  child class.				  parent class }		  ifAbsentPut: [				  | simple |				  simple := (child class name allButFirst: prefix size) asString.				  (self preferredContainers at: simple ifAbsent: [ nil ])					  ifNotNil: [ :property | (property , ':') asSymbol ]					  ifNil: [							  (famixModel metamodel fmTypeOf: child class) allProperties								  detect: [ :p |										  p isContainer and: [												  | t |												  t := p type implementingClass.												  (parent class includesBehavior: t) or: [													  parent class allTraits includes: t ] ] ]								  ifFound: [ :p | (p implementingSelector , ':') asSymbol ]								  ifNone: [ nil ] ] ]! !

!ModelImporter methodsFor: 'importing'!
createAssociations	1 to: relFrom size do: [ :r |			| fromIndex source from to kind |			fromIndex := (relFrom at: r) + 1.			kind := relKindNames at: (relKinds at: r) + 1.			source := self sourceIndexFor: kind from: fromIndex.			from := source ifNotNil: [ entities at: source ].			to := entities at: (relTo at: r) + 1.			(from isNil or: [ to isNil ])				ifTrue: [ self skip: kind ]				ifFalse: [						(siteOffsets at: r) + 1 to: (siteOffsets at: r + 1) do: [ :s |								| assoc |								assoc := self									         newAssociationOf: kind									         from: from									         to: to									         flags: (siteFlags at: s).								assoc ifNil: [ self skip: kind ] ifNotNil: [										associations add: assoc.										"As the Moose exporter: a typing has no anchor; the site is in the file of the original source."										kind = 'declared_type' ifFalse: [												self													queue: assoc													file: (self fileOf: fromIndex)													range: (siteRanges copyFrom: s - 1 * 4 + 1 to: s * 4) ] ] ] ] ]! !

!ModelImporter methodsFor: 'importing'!
createEntities	| classNames |	classNames := self kindClassNames.	entities := Array new: n.	1 to: n do: [ :i |			| simple class entity |			simple := classNames				          at: (kindNames at: (kinds at: i) + 1)				          ifAbsent: [ nil ].			class := simple ifNotNil: [				         Smalltalk					         at: (prefix , simple) asSymbol					         ifAbsent: [ nil ] ].			class ifNotNil: [					entity := class new.					(entity respondsTo: #name:) ifTrue: [ entity name: (names at: i) ].					(((flags at: i) bitAnd: 1) ~= 0 and: [						 entity respondsTo: #isStub: ]) ifTrue: [ entity isStub: true ].					entities at: i put: entity ] ]! !

!ModelImporter methodsFor: 'importing'!
fileOf: anIndex	"File of the entity, or of its closest ancestor that has one."	| i |	i := anIndex.	[ i notNil ] whileTrue: [			(files at: i) ifNotNil: [ :f | ^ f ].			i := (parents at: i) ifNotNil: [ :p | p + 1 ] ].	^ nil! !

!ModelImporter methodsFor: 'importing'!
importModelAt: aPath into: aFamixModel sourceRoot: aRoot	| h |	lib := ModelLibrary uniqueInstance.	famixModel := aFamixModel.	sourceRoot := aRoot asFileReference.	h := lib open: aPath asString.	h isNull ifTrue: [ ^ self error: lib lastError ].	[ self readColumns: h ] ensure: [ lib free: h ].	containerCache := Dictionary new.	skipped := Dictionary new.	associations := OrderedCollection new.	anchorQueue := Dictionary new.	self createEntities.	self linkContainers.	self computeFileTypes.	self queueEntityAnchors.	self createAssociations.	self resolveAnchors.	famixModel silentlyAddAll: (entities select: #notNil) , associations.	^ self report! !

!ModelImporter methodsFor: 'importing'!
importerIndexOf: anIndex	"An import is made by the first top-level type of the file when the source is the file itself."	(entities at: anIndex) ifNotNil: [ ^ anIndex ].	^ fileTypes		  at: ((files at: anIndex) ifNil: [ names at: anIndex ])		  ifAbsent: [ nil ]! !

!ModelImporter methodsFor: 'importing'!
isCallableAt: anIndex	^ #( 'method' 'constructor' 'function' ) includes:		  (kindArchetypes at: (kinds at: anIndex) + 1)! !

!ModelImporter methodsFor: 'importing'!
isInterfaceLike: anEntity	^ {		  (prefix , 'Interface') asSymbol.		  (prefix , 'AnnotationType') asSymbol } includes:		  anEntity class name! !

!ModelImporter methodsFor: 'importing'!
isNamed: aSimpleName in: anEntity	^ anEntity class name = (prefix , aSimpleName) asSymbol! !

!ModelImporter methodsFor: 'importing'!
isTypeEntity: anEntity	"A type as Famix associations target it (typing, reference); the FM3 traits of some types (interfaces) do not say so."	^ #( 'Class' 'Interface' 'Enum' 'AnnotationType' 'TypeParameter' )		  anySatisfy: [ :simple | self isNamed: simple in: anEntity ]! !

!ModelImporter methodsFor: 'accessing'!
kindClassNames	"Model kind name -> simple FM3 class name. Data, per target language family."	^ Dictionary newFromPairs:		  #( 'namespace' 'Package' 'class' 'Class' 'record' 'Class' 'interface'		     'Interface' 'annotation_type' 'AnnotationType' 'enum' 'Enum'		     'method' 'Method' 'constructor' 'Method' 'field' 'Attribute'		     'constant' 'Attribute' 'enum_constant' 'EnumValue' 'parameter'		     'Parameter' 'local_variable' 'LocalVariable' 'type_parameter'		     'TypeParameter' 'annotation' 'AnnotationInstance'		     'comment' 'Comment' )! !

!ModelImporter methodsFor: 'importing'!
linkContainers	1 to: n do: [ :i |			| child parentIndex parent selector |			child := entities at: i.			parentIndex := parents at: i.			(child notNil and: [ parentIndex notNil ]) ifTrue: [					parent := entities at: parentIndex + 1.					parent ifNotNil: [							selector := self containerSelectorFor: child parent: parent.							selector ifNotNil: [ child perform: selector with: parent ] ] ] ]! !

!ModelImporter methodsFor: 'importing'!
newAssociationOf: aKind from: from to: to flags: aFlagByte	| spec class toOk |	spec := self associationSpecFor: aKind from: from to: to.	spec ifNil: [ ^ nil ].	class := self classNamed: spec first.	toOk := (#( declaredType referredEntity ) includes: spec third)		        ifTrue: [ self isTypeEntity: to ]		        ifFalse: [ self accepts: to as: spec third of: class ].	((self accepts: from as: spec second of: class) and: [ toOk ])		ifFalse: [ ^ nil ].	spec first = 'Invocation' ifTrue: [			^ class new				  sender: from;				  addCandidate: to;				  yourself ].	spec first = 'Access' ifTrue: [			^ class new				  accessor: from;				  variable: to;				  isWrite: (aFlagByte bitAnd: 2) ~= 0;				  yourself ].	spec first = 'EntityTyping' ifTrue: [			^ class new				  typedEntity: from;				  declaredType: to;				  yourself ].	spec first = 'Reference' ifTrue: [			^ class new				  referencer: from;				  referredEntity: to;				  yourself ].	spec first = 'Import' ifTrue: [			^ class new				  importingEntity: from;				  importedEntity: to;				  yourself ].	spec first = 'Implementation' ifTrue: [			^ class new				  implementingClass: from;				  interface: to;				  yourself ].	^ class new		  subclass: from;		  superclass: to;		  yourself! !

!ModelImporter methodsFor: 'importing'!
ownerIndexOf: anIndex	"The entity (or its nearest callable ancestor) that sends calls and makes accesses."	| i |	((entities at: anIndex) notNil and: [ self isCallableAt: anIndex ])		ifTrue: [ ^ anIndex ].	i := parents at: anIndex.	[ i notNil ] whileTrue: [			((entities at: i + 1) notNil and: [ self isCallableAt: i + 1 ])				ifTrue: [ ^ i + 1 ].			i := parents at: i + 1 ].	^ (entities at: anIndex) ifNotNil: [ anIndex ]! !

!ModelImporter methodsFor: 'importing'!
preferredContainers	"Simple FM3 class name -> container property, when the FM3 types alone are ambiguous. Mirrors what the Moose exporter writes (types go in typeContainer); anything else is found through the FM3 metamodel."	^ Dictionary newFromPairs:		  #( 'Class' #typeContainer 'Exception' #typeContainer		     'Interface' #typeContainer 'Enum' #typeContainer 'TypeParameter'		     #typeContainer )! !

!ModelImporter methodsFor: 'accessing'!
prefix: aString	prefix := aString! !

!ModelImporter methodsFor: 'importing'!
queue: anElement file: aFile range: aQuadruple	aFile ifNil: [ ^ self skip: 'anchor:no-file' ].	(anchorQueue at: aFile ifAbsentPut: [ OrderedCollection new ]) add:		{			anElement.			aQuadruple }! !

!ModelImporter methodsFor: 'importing'!
queueEntityAnchors	1 to: n do: [ :i |			((entities at: i) notNil and: [ ((flags at: i) bitAnd: 2) ~= 0 ])				ifTrue: [						self							queue: (entities at: i)							file: (files at: i)							range: (ranges copyFrom: i - 1 * 4 + 1 to: i * 4) ] ]! !

!ModelImporter methodsFor: 'importing'!
readColumns: h	| relCount siteCount kindCount relKindCount |	n := lib entityCount: h.	relCount := lib relationCount: h.	siteCount := lib siteCount: h.	kindCount := lib kindCount: h.	relKindCount := lib relationKindCount: h.	names := lib stringColumn: (lib entityNames: h) count: n.	files := lib stringColumn: (lib entityFiles: h) count: n.	parents := lib uint32Column: (lib entityParents: h) count: n.	kinds := lib uint32Column: (lib entityKinds: h) count: n.	flags := lib uint32Column: (lib entityFlags: h) count: n.	ranges := lib uint32Column: (lib entityRanges: h) count: n * 4.	kindNames := lib stringColumn: (lib kindNames: h) count: kindCount.	kindArchetypes := lib		                  stringColumn: (lib kindArchetypes: h)		                  count: kindCount.	relFrom := lib uint32Column: (lib relationFrom: h) count: relCount.	relTo := lib uint32Column: (lib relationTo: h) count: relCount.	relKinds := lib uint32Column: (lib relationKinds: h) count: relCount.	siteOffsets := lib		               uint32Column: (lib relationSiteOffsets: h)		               count: relCount + 1.	siteRanges := lib		              uint32Column: (lib siteRanges: h)		              count: siteCount * 4.	siteFlags := lib uint8Column: (lib siteFlags: h) count: siteCount.	relKindNames := lib		                stringColumn: (lib relationKindNames: h)		                count: relKindCount! !

!ModelImporter methodsFor: 'accessing'!
report	^ {		  (#entities -> (entities count: #notNil)).		  (#associations -> associations size).		  (#skippedRelations -> skipped) }! !

!ModelImporter methodsFor: 'importing'!
resolveAnchors	anchorQueue keysAndValuesDo: [ :file :items |			| path spans |			path := (sourceRoot resolve: file) pathString.			spans := lib charOffsetsIn: path ranges: (items collect: #second).			spans ifNil: [ self skip: 'anchor:unreadable-file' ] ifNotNil: [					items with: spans do: [ :item :span |							| anchor |							anchor := (self classNamed: 'IndexedFileAnchor') new								          fileName: path;								          startPos: span first;								          endPos: span last;								          yourself.							item first sourceAnchor: anchor.							associations add: anchor ] ] ]! !

!ModelImporter methodsFor: 'importing'!
skip: aKey	skipped at: aKey update: [ :c | c + 1 ] initial: [ 1 ]! !

!ModelImporter methodsFor: 'importing'!
sourceIndexFor: aKind from: anIndex	(#( 'calls' 'accesses' 'references' ) includes: aKind) ifTrue: [		^ self ownerIndexOf: anIndex ].	aKind = 'imports' ifTrue: [ ^ self importerIndexOf: anIndex ].	^ (entities at: anIndex) ifNotNil: [ anIndex ]! !
